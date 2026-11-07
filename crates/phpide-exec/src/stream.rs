//! Spawn com saída incremental — `T-RUN-001-02` (RUN-001).
//!
//! [`run`]/[`spawn_job`] agregam stdout/stderr ao final; painéis de execução
//! precisam de **linhas enquanto o processo roda** (Run tool window, PHPUnit
//! com árvore progressiva). [`spawn_stream`] spawna com o mesmo contrato de
//! [`CommandSpec`] (vetor de args, cwd, env, grupo de processos próprio) e
//! entrega eventos por canal: linha de stdout, linha de stderr e exit code.
//! Stop mata o grupo inteiro (NFR-003-03); variáveis de ambiente nunca vão
//! para a descrição pública (redaction, RUN-001/T-RUN-001-05).

use std::io::{BufRead, BufReader};
use std::process::Child;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use phpide_core::{Component, IdeError};

use crate::process::{build_command, kill_process_tree, CommandSpec};

/// Evento incremental de um processo em execução.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// Linha de stdout (sem terminador).
    Line(String),
    /// Linha de stderr (sem terminador).
    ErrLine(String),
    /// Processo encerrado; `None` = morto por sinal/Stop (sem exit code).
    Exit(Option<i32>),
}

/// Handle de um processo com saída em streaming.
///
/// A UI faz poll por frame com [`StreamingChild::drain`]; o Stop é imediato
/// e alcança descendentes (grupo de processos, Unix).
pub struct StreamingChild {
    rx: Receiver<StreamEvent>,
    child: Arc<Mutex<Child>>,
    /// Descrição segura para exibição: programa + args (nunca env).
    pub description: String,
}

impl StreamingChild {
    /// Drena todos os eventos disponíveis agora (não bloqueia).
    pub fn drain(&self) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            out.push(ev);
        }
        out
    }

    /// `true` enquanto o processo não foi reaped.
    pub fn is_running(&self) -> bool {
        self.child
            .lock()
            .map(|mut c| c.try_wait().ok().flatten().is_none())
            .unwrap_or(false)
    }

    /// Encerra o grupo de processos (Stop — Ctrl+F2 no painel Run).
    pub fn stop(&self) {
        if let Ok(mut child) = self.child.lock() {
            kill_process_tree(&mut child, true);
        }
    }
}

/// Spawna `spec` com stdout/stderr line-buffered chegando por eventos.
pub fn spawn_stream(spec: CommandSpec) -> Result<StreamingChild, IdeError> {
    let description = format!("{} {}", spec.program, spec.args.join(" "));
    let mut child = build_command(&spec)
        .spawn()
        .map_err(|e| IdeError::spawn(Component::Exec, std::path::Path::new(&spec.program), &e))?;
    let streams: [Option<Box<dyn std::io::Read + Send>>; 2] = [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    ];
    let child = Arc::new(Mutex::new(child));
    let (tx, rx) = channel::<StreamEvent>();

    let mut pump_handles = Vec::new();
    for (pipe, is_err) in streams.into_iter().zip([false, true]) {
        let Some(pipe) = pipe else { continue };
        let tx = tx.clone();
        pump_handles.push(std::thread::spawn(move || {
            let reader = BufReader::new(pipe);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                let ev = if is_err {
                    StreamEvent::ErrLine(line)
                } else {
                    StreamEvent::Line(line)
                };
                if tx.send(ev).is_err() {
                    break;
                }
            }
        }));
    }

    // Worker de wait: reaped o filho, junta os pumps (o EOF do pipe vem com a
    // morte do processo) e SÓ ENTÃO publica Exit — toda linha chega ao
    // receptor antes do evento de saída, sem corrida no drain da UI.
    let waiter_child = child.clone();
    std::thread::spawn(move || loop {
        let status = waiter_child.lock().map(|mut c| c.try_wait().ok().flatten());
        match status {
            Ok(Some(status)) => {
                for handle in pump_handles {
                    let _ = handle.join();
                }
                let _ = tx.send(StreamEvent::Exit(status.code()));
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            // Mutex envenenado: nada mais a fazer.
            Err(_) => break,
        }
    });

    Ok(StreamingChild {
        rx,
        child,
        description,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn streams_lines_incrementally_and_exits() {
        let spec = CommandSpec::new(
            "sh",
            vec![
                "-c".into(),
                "echo um; sleep 0.2; echo dois; echo err 1>&2".into(),
            ],
        );
        let child = spawn_stream(spec).unwrap();
        assert!(child.is_running());
        let mut lines = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut exit = None;
        while Instant::now() < deadline {
            for ev in child.drain() {
                match ev {
                    StreamEvent::Line(l) => lines.push(l),
                    StreamEvent::ErrLine(l) => lines.push(format!("err:{l}")),
                    StreamEvent::Exit(c) => exit = Some(c),
                }
            }
            if exit.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(lines, vec!["um", "dois", "err:err"]);
        assert_eq!(exit, Some(Some(0)));
        assert!(!child.is_running());
    }

    #[test]
    fn stop_kills_streaming_process() {
        let spec = CommandSpec::new("sh", vec!["-c".into(), "sleep 30".into()]);
        let child = spawn_stream(spec).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        child.stop();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut exit = None;
        while Instant::now() < deadline {
            for ev in child.drain() {
                if let StreamEvent::Exit(c) = ev {
                    exit = Some(c);
                }
            }
            if exit.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(exit, Some(None), "morto por Stop: sem exit code");
        assert!(!child.is_running());
    }

    #[test]
    fn description_never_contains_env() {
        // A descrição pública do processo leva programa + args, nunca env:
        // segredos de configuração não podem vazar para a tool window (RUN-001).
        let spec = CommandSpec::new("sh", vec!["-c".into(), "true".into()])
            .env("API_KEY", "super-secreta-123");
        let child = spawn_stream(spec).unwrap();
        assert!(
            !child.description.contains("super-secreta-123"),
            "{}",
            child.description
        );
        child.stop();
    }
}
