//! Spawn padronizado, timeout, Stop e cleanup de filhos — `T-NFR-003-02`,
//! `T-NFR-003-03` (NFR-003).
//!
//! - Argumentos sempre como **vetor** — nada de concatenar entrada do
//!   usuário em comando shell (exceto o Terminal explícito, que é outro
//!   canal e vive na UI);
//! - stdout e stderr capturados **separadamente**, com exit code próprio
//!   (`None` quando o processo foi morto, por timeout/cancel/Stop);
//! - timeout e cancelamento encerram o **grupo de processos** no Unix
//!   (filhos não ficam órfãos — ex.: `php script.php` que spawna mais php);
//! - integração com [`JobSystem`] da UI: resultados por canal, cancelamento
//!   cooperativo, descarte por [`JobRegistry`] (NFR-001).

use phpide_core::jobs::{JobContext, JobHandle, JobSystem};
use phpide_core::{Component, IdeError};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Especificação completa de um spawn (NFR-003-02).
pub struct CommandSpec {
    /// Programa (caminho absoluto ou nome no PATH).
    pub program: String,
    /// Argumentos como vetor — nunca shell-concat.
    pub args: Vec<String>,
    /// Diretório de trabalho (None = cwd do processo pai).
    pub cwd: Option<PathBuf>,
    /// Variáveis de ambiente adicionais (nunca logadas — ver redact).
    pub env: Vec<(String, String)>,
    /// Tempo máximo de execução (None = sem limite).
    pub timeout: Option<Duration>,
    /// Encerrar o grupo de processos inteiro no stop (default: true no
    /// Unix; sem efeito em outras plataformas).
    pub kill_tree: bool,
}

impl CommandSpec {
    pub fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        CommandSpec {
            program: program.into(),
            args,
            cwd: None,
            env: Vec::new(),
            timeout: None,
            kill_tree: true,
        }
    }

    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub fn timeout(mut self, limit: Duration) -> Self {
        self.timeout = Some(limit);
        self
    }
}

/// Resultado padronizado de um processo (NFR-003-02).
#[derive(Debug)]
pub struct Completed {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// Código de saída; `None` = morto (timeout/cancel/Stop) ou sinal.
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
    pub duration: Duration,
}

impl Completed {
    /// stdout como texto (lossy), para exibição em painéis.
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// stderr como texto (lossy).
    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// Encerra o processo — e o grupo inteiro no Unix (NFR-003-03: filhos não
/// ficam órfãos). Público para reutilizadores (ex.: Stop do painel Run).
pub fn kill_process_tree(child: &mut Child, kill_tree: bool) {
    #[cfg(unix)]
    if kill_tree {
        // O filho foi colocado num novo grupo (setsid no pre_exec): sinal ao
        // grupo pega descendentes diretos que ainda compartilham o grupo.
        let pid = child.id() as i32;
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
        return;
    }
    let _ = kill_tree;
    let _ = child.kill();
}

/// Configura o spawn: pipes, cwd, env e (Unix) grupo de processos próprio.
fn build_command(spec: &CommandSpec) -> Command {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    #[cfg(unix)]
    if spec.kill_tree {
        use std::os::unix::process::CommandExt;
        // Novo session id por processo: o grupo pode ser morto de uma vez
        // no stop/timeout (cleanup de processos-filhos, NFR-003-03).
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1
                    && std::io::Error::last_os_error().raw_os_error() != Some(libc::EPERM)
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    cmd
}

/// Núcleo de execução: spawna, bombeia stdout/stderr em threads próprias e
/// aplica cancel/timeout matando o grupo.
fn drive(
    spec: CommandSpec,
    is_cancelled: Option<&dyn Fn() -> bool>,
    deadline: Option<Instant>,
) -> Result<Completed, IdeError> {
    let started = Instant::now();
    let kill_tree = spec.kill_tree;
    // Retry de spawn em ETXTBSY/EAGAIN: executar um binário recém-criado
    // pode colidir com a liberação do inode (transiente sob carga).
    let mut attempt = 0;
    let mut child = loop {
        match build_command(&spec).spawn() {
            Ok(child) => break child,
            Err(e)
                if attempt < 3
                    && matches!(
                        e.kind(),
                        std::io::ErrorKind::ExecutableFileBusy | std::io::ErrorKind::WouldBlock
                    ) =>
            {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(30 * attempt));
            }
            Err(e) => {
                return Err(IdeError::spawn(
                    Component::Exec,
                    PathBuf::from(&spec.program).as_path(),
                    &e,
                ));
            }
        }
    };

    let stdout_owned = child.stdout.take();
    let stderr_owned = child.stderr.take();
    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
    let (err_tx, err_rx) = mpsc::channel::<Vec<u8>>();
    let pump_out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = stdout_owned {
            let _ = p.read_to_end(&mut buf);
        }
        let _ = out_tx.send(buf);
    });
    let pump_err = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = stderr_owned {
            let _ = p.read_to_end(&mut buf);
        }
        let _ = err_tx.send(buf);
    });

    // Consome rx + handles (join pega ownership) exatamente uma vez por caminho.
    macro_rules! finish {
        ($code:expr, $timed_out:expr, $cancelled:expr) => {{
            let stdout = out_rx.recv().unwrap_or_default();
            let stderr = err_rx.recv().unwrap_or_default();
            let _ = pump_out.join();
            let _ = pump_err.join();
            return Ok(Completed {
                stdout,
                stderr,
                exit_code: $code,
                timed_out: $timed_out,
                cancelled: $cancelled,
                duration: started.elapsed(),
            });
        }};
    }

    loop {
        match child.try_wait() {
            Ok(Some(status)) => finish!(status.code(), false, false),
            Ok(None) => {}
            Err(e) => {
                kill_process_tree(&mut child, kill_tree);
                let _ = pump_out.join();
                let _ = pump_err.join();
                return Err(IdeError::from_io(
                    Component::Exec,
                    "wait",
                    &spec.program,
                    &e,
                ));
            }
        }
        if is_cancelled.map(|f| f()).unwrap_or(false) {
            kill_process_tree(&mut child, kill_tree);
            let _ = child.wait();
            finish!(None, false, true)
        }
        if let Some(dl) = deadline {
            if Instant::now() >= dl {
                kill_process_tree(&mut child, kill_tree);
                let _ = child.wait();
                finish!(None, true, false)
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Executa `spec` bloqueando a thread chamadora (para testes/scripts;
/// a UI deve usar [`spawn_job`]).
pub fn run(spec: CommandSpec) -> Result<Completed, IdeError> {
    let deadline = spec.timeout.map(|t| Instant::now() + t);
    drive(spec, None, deadline)
}

/// Dispara `spec` num worker do [`JobSystem`]: a UI faz poll por frame e o
/// Stop é `handle.cancel()` — o worker mata o grupo e devolve
/// `Completed { cancelled: true }` (NFR-001 + NFR-003-03).
pub fn spawn_job(system: &JobSystem, spec: CommandSpec) -> JobHandle<Completed> {
    let deadline = spec.timeout.map(|t| Instant::now() + t);
    system.spawn(move |ctx: &JobContext| drive(spec, Some(&|| ctx.is_cancelled()), deadline))
}

#[cfg(test)]
mod tests {
    use super::*;
    use phpide_core::jobs::JobOutcome;
    use phpide_core::ErrorKind;

    fn sh(args: &[&str]) -> CommandSpec {
        CommandSpec::new("sh", args.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn run_captures_stdout_stderr_and_exit_code() {
        let out = run(sh(&["-c", "echo saída; echo erro 1>&2; exit 3"])).unwrap();
        assert_eq!(out.stdout_text().trim(), "saída");
        assert_eq!(out.stderr_text().trim(), "erro");
        assert_eq!(out.exit_code, Some(3));
        assert!(!out.timed_out && !out.cancelled);
    }

    #[test]
    fn missing_program_is_actionable_not_found() {
        let e = run(CommandSpec::new(
            "phpide-ferramenta-que-nao-existe-xyz",
            vec![],
        ))
        .unwrap_err();
        assert_eq!(e.kind, ErrorKind::NotFound);
        assert!(e.action.contains("instale") || e.action.contains("Settings"));
    }

    #[test]
    fn timeout_kills_and_reports() {
        let out = run(sh(&["-c", "sleep 2"]).timeout(Duration::from_millis(150))).unwrap();
        assert!(out.timed_out, "timeout reportado separadamente");
        assert_eq!(out.exit_code, None, "morto: sem exit code");
        assert!(
            out.duration < Duration::from_millis(1200),
            "processo encerrado no timeout ({:?})",
            out.duration
        );
    }

    #[test]
    fn spawn_job_cancel_stops_process() {
        let sys = JobSystem::new();
        let mut handle = spawn_job(&sys, sh(&["-c", "sleep 5"]));
        std::thread::sleep(Duration::from_millis(50));
        handle.cancel();
        let outcome = handle.wait(Duration::from_secs(5));
        match outcome {
            Some(JobOutcome::Ok(c)) => {
                assert!(c.cancelled, "cancelamento sinalizado no resultado");
                assert_eq!(c.exit_code, None);
            }
            other => panic!("esperava Completed cancelado, veio {other:?}"),
        }
    }

    #[test]
    fn spawn_job_reports_completion() {
        let sys = JobSystem::new();
        let mut handle = spawn_job(&sys, sh(&["-c", "echo ok"]));
        match handle.wait(Duration::from_secs(5)) {
            Some(JobOutcome::Ok(c)) => {
                assert_eq!(c.stdout_text().trim(), "ok");
                assert_eq!(c.exit_code, Some(0));
            }
            other => panic!("esperava conclusão, veio {other:?}"),
        }
    }

    #[test]
    fn kill_tree_reaches_grandchildren_on_unix() {
        // sh spawna um sleep neto; sem kill de grupo o neto sobreviveria.
        if !cfg!(unix) {
            return;
        }
        let marker = std::env::temp_dir().join(format!("phpide-killtree-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let script = format!("(sleep 30) & echo $! > {}; wait", marker.display());
        let out = run(sh(&["-c", &script]).timeout(Duration::from_millis(200))).unwrap();
        assert!(out.timed_out);
        // O filho direto (sh) morreu; o neto (sleep) estava no mesmo grupo.
        std::thread::sleep(Duration::from_millis(100));
        if marker.exists() {
            let pid: i32 = std::fs::read_to_string(&marker)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            // kill(-0, pid) falha com ESRCH se o processo não existe.
            let alive = unsafe { libc::kill(pid, 0) } == 0;
            assert!(!alive, "processo-filho (neto) também encerrado");
            let _ = std::fs::remove_file(&marker);
        }
    }
}
