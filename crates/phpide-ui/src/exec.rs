//! Executor de comandos (Estágio 4).
//!
//! Roda `php`, `composer`, `phpunit` e plugins em thread separada; stdout e
//! stderr chegam por canal para o painel Execução (mesmo padrão da busca:
//! nunca bloqueia a thread de UI).

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;

pub enum ExecMsg {
    Line(String),
    Done(i32),
    /// Processo filho, logo após o spawn (para Stop — Ctrl+F2).
    Child(Arc<Mutex<Child>>),
}

#[derive(Default)]
pub struct ExecState {
    pub title: String,
    pub output: String,
    rx: Option<Receiver<ExecMsg>>,
    pub running: bool,
    /// Handle do processo filho (Stop mata o grupo inteiro, NFR-003-03).
    child: Option<Arc<Mutex<Child>>>,
}

impl ExecState {
    pub fn new() -> Self {
        ExecState {
            title: "Run".to_string(),
            output: String::new(),
            rx: None,
            running: false,
            child: None,
        }
    }

    pub fn run(&mut self, title: String, cmd: String, args: Vec<String>, cwd: PathBuf) {
        self.run_env(title, cmd, args, cwd, Vec::new());
    }

    /// Como `run`, mas com variáveis de ambiente (ex.: XDEBUG_CONFIG no debug).
    pub fn run_env(
        &mut self,
        title: String,
        cmd: String,
        args: Vec<String>,
        cwd: PathBuf,
        env_vars: Vec<(String, String)>,
    ) {
        if self.running {
            return;
        }
        self.title = title;
        self.output.clear();
        self.running = true;
        let (tx, rx) = channel();
        self.rx = Some(rx);
        thread::spawn(move || {
            let mut command = Command::new(&cmd);
            command
                .args(&args)
                .current_dir(&cwd)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            for (k, v) in env_vars {
                command.env(k, v);
            }
            let mut child = match command.spawn() {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(ExecMsg::Line(format!("falha ao iniciar {cmd}: {e}")));
                    let _ = tx.send(ExecMsg::Done(-1));
                    return;
                }
            };
            let streams: [(Option<Box<dyn Read + Send>>, bool); 2] = [
                (
                    child
                        .stdout
                        .take()
                        .map(|s| Box::new(s) as Box<dyn Read + Send>),
                    false,
                ),
                (
                    child
                        .stderr
                        .take()
                        .map(|s| Box::new(s) as Box<dyn Read + Send>),
                    true,
                ),
            ];
            for (stream, is_err) in streams {
                let Some(stream) = stream else { continue };
                let tx = tx.clone();
                thread::spawn(move || {
                    let mut reader = BufReader::new(stream);
                    let mut line = String::new();
                    while reader.read_line(&mut line).unwrap_or(0) > 0 {
                        let out = if is_err {
                            format!("[stderr] {}", line.trim_end())
                        } else {
                            line.trim_end().to_string()
                        };
                        let _ = tx.send(ExecMsg::Line(out));
                        line.clear();
                    }
                });
            }
            // Compartilha o handle: a UI pode fazer Stop (kill do grupo) a
            // qualquer momento; o worker detecta o término por try_wait.
            let shared = Arc::new(Mutex::new(child));
            let worker_child = shared.clone();
            if tx.send(ExecMsg::Child(shared)).is_err() {
                return;
            }
            loop {
                let status = worker_child.lock().unwrap().try_wait();
                match status {
                    Ok(Some(s)) => {
                        let _ = tx.send(ExecMsg::Done(s.code().unwrap_or(-1)));
                        break;
                    }
                    Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
                    Err(_) => {
                        let _ = tx.send(ExecMsg::Done(-1));
                        break;
                    }
                }
            }
        });
    }

    /// Stop (Ctrl+F2): encerra o grupo de processos do filho (Linux) — os
    /// netos não ficam órfãos (NFR-003-03: cleanup de processos-filhos).
    pub fn stop(&mut self) {
        if let Some(shared) = self.child.take() {
            if let Ok(mut child) = shared.lock() {
                phpide_exec::kill_process_tree(&mut child, true);
            }
            self.output.push_str("\n[parado pelo usuário]");
        }
    }

    /// Encerra qualquer processo em andamento (troca de projeto / fechamento
    /// da IDE, NFR-001-04).
    pub fn stop_if_running(&mut self) {
        if self.running || self.child.is_some() {
            self.stop();
        }
    }

    pub fn poll(&mut self) {
        let mut lines = Vec::new();
        let mut done = None;
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    ExecMsg::Line(l) => lines.push(l),
                    ExecMsg::Done(c) => done = Some(c),
                    ExecMsg::Child(child) => self.child = Some(child),
                }
            }
        }
        for l in lines {
            self.push_line(&l);
        }
        if let Some(code) = done {
            self.rx = None;
            self.running = false;
            self.child = None;
            self.output.push_str(&format!("\n[exit {code}]"));
        }
    }

    /// Acrescenta uma linha vinda de outra fonte (ex.: sessão DBGp).
    pub fn push_line(&mut self, line: &str) {
        if self.output.len() > 400_000 {
            self.output.clear();
            self.output.push_str("[saída truncada]\n");
        }
        self.output.push_str(line);
        self.output.push('\n');
    }
}

/// Lê os scripts de composer.json (se existir): (nome, descrição).
pub fn composer_scripts(root: &std::path::Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(root.join("composer.json")) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    value["scripts"]
        .as_object()
        .map(|obj| {
            obj.iter()
                .map(|(k, v)| {
                    let desc = v
                        .as_str()
                        .map(String::from)
                        .or_else(|| {
                            v.as_array()
                                .and_then(|a| a.first())
                                .and_then(|x| x.as_str())
                                .map(String::from)
                        })
                        .unwrap_or_default();
                    (k.clone(), desc)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Resolução configurável de ferramentas (NFR-003-01): variável de ambiente
/// da IDE > caminho em Settings > PATH. Erros vêm com ação recomendada
/// (NFR-005) prontos para a status bar.
pub fn resolve_tool(
    name: &str,
    label: &str,
    env_key: &str,
    setting_path: &str,
) -> Result<phpide_exec::ToolInfo, phpide_core::IdeError> {
    use phpide_exec::ToolRequest;
    let explicit = std::env::var(env_key)
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| (!setting_path.is_empty()).then(|| PathBuf::from(setting_path)));
    let mut req = ToolRequest::new(name, label);
    if let Some(path) = explicit {
        req = req.explicit(path);
    }
    phpide_exec::discover(&req)
}

/// Resolve o interpretador PHP (PHPIDE_PHP > settings.php_path > PATH).
pub fn resolve_php(settings: &crate::settings::Settings) -> Result<String, phpide_core::IdeError> {
    resolve_tool("php", "PHP", "PHPIDE_PHP", &settings.php_path)
        .map(|info| info.program.to_string_lossy().into_owned())
}

/// Resolve o Composer (PHPIDE_COMPOSER > settings.composer_path > PATH).
pub fn resolve_composer(
    settings: &crate::settings::Settings,
) -> Result<String, phpide_core::IdeError> {
    resolve_tool(
        "composer",
        "Composer",
        "PHPIDE_COMPOSER",
        &settings.composer_path,
    )
    .map(|info| info.program.to_string_lossy().into_owned())
}

/// Monta o `BuildContext` do domínio de execução a partir das ferramentas
/// já resolvidas (NFR-003-01: configuração explícita antes do PATH).
pub fn build_context(settings: &crate::settings::Settings, php: &str) -> phpide_run::BuildContext {
    phpide_run::BuildContext {
        php: php.to_string(),
        composer: resolve_composer(settings).ok(),
        phpunit: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn runs_php_and_captures_output() {
        let mut exec = ExecState::new();
        exec.run(
            "teste".into(),
            std::env::var("PHPIDE_PHP").unwrap_or_else(|_| "php".into()),
            vec!["-r".into(), "echo 'ok-exec';".into()],
            std::env::temp_dir(),
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        while exec.running && Instant::now() < deadline {
            exec.poll();
            std::thread::sleep(Duration::from_millis(50));
        }
        exec.poll();
        assert!(exec.output.contains("ok-exec"), "saída: {}", exec.output);
        assert!(!exec.running);
    }

    #[test]
    fn composer_scripts_empty_without_file() {
        assert!(composer_scripts(&PathBuf::from("/caminho/que/nao/existe")).is_empty());
    }
}
