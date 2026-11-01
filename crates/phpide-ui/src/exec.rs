//! Executor de comandos (Estágio 4).
//!
//! Roda `php`, `composer`, `phpunit` e plugins em thread separada; stdout e
//! stderr chegam por canal para o painel Execução (mesmo padrão da busca:
//! nunca bloqueia a thread de UI).

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::thread;

pub enum ExecMsg {
    Line(String),
    Done(i32),
    /// PID do filho, logo após o spawn (para Stop — Ctrl+F2).
    ChildPid(u32),
}

#[derive(Default)]
pub struct ExecState {
    pub title: String,
    pub output: String,
    rx: Option<Receiver<ExecMsg>>,
    pub running: bool,
    /// PID do processo filho (para Stop — Ctrl+F2).
    child_id: Option<u32>,
}

impl ExecState {
    pub fn new() -> Self {
        ExecState {
            title: "Run".to_string(),
            output: String::new(),
            rx: None,
            running: false,
            child_id: None,
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
            let child_id = child.id();
            let _ = tx.send(ExecMsg::ChildPid(child_id));
            let streams: [(Option<Box<dyn Read + Send>>, bool); 2] = [
                (
                    child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
                    false,
                ),
                (
                    child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
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
            let code = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
            let _ = tx.send(ExecMsg::Done(code));
        });
    }

    /// Stop (Ctrl+F2): envia SIGTERM ao processo filho (Linux).
    pub fn stop(&mut self) {
        if let Some(pid) = self.child_id.take() {
            let _ = std::process::Command::new("kill")
                .arg(pid.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            self.output.push_str("\n[parado pelo usuário]");
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
                    ExecMsg::ChildPid(pid) => self.child_id = Some(pid),
                }
            }
        }
        for l in lines {
            self.push_line(&l);
        }
        if let Some(code) = done {
            self.rx = None;
            self.running = false;
            self.child_id = None;
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
pub fn composer_scripts(root: &PathBuf) -> Vec<(String, String)> {
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
                            v.as_array().and_then(|a| a.first()).and_then(|x| x.as_str()).map(String::from)
                        })
                        .unwrap_or_default();
                    (k.clone(), desc)
                })
                .collect()
        })
        .unwrap_or_default()
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
