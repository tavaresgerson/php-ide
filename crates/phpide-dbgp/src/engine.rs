//! Engine de debug — máquina de estados DBGp completa (Estágio 8).
//!
//! Uma thread por listener: aceita a conexão do Xdebug (com IDE key e
//! associação ao projeto), sincroniza breakpoints, executa comandos de
//! continuação e emite [`EngineEvent`] para a UI por canal. Comandos da UI
//! chegam por [`EngineCommand`].
//!
//! - **Detach/Stop confiáveis** (T-RUN-DBG-001-04): o socket mantém um
//!   "cloner" compartilhado — Detach/Stop durante um `run` bloqueado faz
//!   shutdown do socket, o `continue_exec` desbloqueia na hora e o estado
//!   final chega como evento. Stop (Ctrl+F2) encerra também o processo PHP
//!   (a UI mata o grupo via RunManager; aqui o socket é fechado).
//! - **Cleanup**: shutdown da engine fecha socket e listener; o processo PHP
//!   é responsabilidade de quem o spawnou (RunManager, NFR-001-04).

use std::io;
use std::net::TcpStream;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::breakpoint::{BreakpointStore, SyncEvent};
use crate::listener::{AcceptOutcome, DebugListener, InitInfo, ListenerConfig};
use crate::mapping::{ConnectionMatch, PathMappings};
use crate::proto::DbgpSession;
use crate::session::{ContinueCommand, Frame, StopReason};

/// Como a sessão de debug terminou.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionEnd {
    /// Detach pedido: engine PHP segue (socket fechado).
    Detached,
    /// Script chegou ao fim naturalmente.
    Finished,
    /// Stop pelo usuário.
    Stopped,
    /// Socket morreu sem detach (processo morto/crash).
    ConnectionLost(String),
}

/// Eventos para a tool window Debug.
#[derive(Debug)]
pub enum EngineEvent {
    /// Listener ativo na porta.
    Listening { port: u16 },
    /// Conexão DBGp aceita (após filtro de IDE key) e breakpoints sincronizados.
    Connected {
        init: InitInfo,
        matched: ConnectionMatch,
        synced: Vec<SyncEvent>,
    },
    /// Resultado de re-sincronização (breakpoints mudaram com sessão ativa).
    BreakpointsSynced(Vec<SyncEvent>),
    /// Execução parou (breakpoint/step/exception); frames incluem o topo.
    Stopped {
        reason: StopReason,
        frames: Vec<Frame>,
    },
    /// Comando de continuação aceito (a UI pode limpar watches/inline values).
    Continued(ContinueCommand),
    /// Sessão encerrada.
    Ended(SessionEnd),
    /// Falha de I/O ou protocolo (mensagem curta; detalhes no log da UI).
    Error(String),
    /// Resultado de Evaluate por frame.
    Evaluated {
        expression: String,
        result: Result<Option<crate::session::Variable>, String>,
    },
    /// Resultado da reavaliação de watches na parada.
    WatchValues(Vec<crate::session::WatchValue>),
    /// Variáveis do frame selecionado.
    FrameVariables(Vec<crate::session::Variable>),
}

/// Comandos da UI para a engine.
#[derive(Debug)]
pub enum EngineCommand {
    /// Breakpoints mudaram: re-sincroniza com a sessão ativa (o store é
    /// compartilhado por `Arc<Mutex>` com a UI — gutter e lista).
    UpdateBreakpoints,
    /// Resume ou stepping (F9/F7/F8/Shift+F8).
    Continue(ContinueCommand),
    /// Detach: segue sem debug, socket fechado.
    Detach,
    /// Stop: encerra a sessão (a UI mata o processo PHP).
    Stop,
    /// Avalia expressão associada ao frame (Evaluate Expression, Alt+F8).
    Evaluate { expression: String, depth: u32 },
    /// Reavalía todos os watches no frame (resultados por parada).
    EvaluateWatches {
        ids: Vec<u64>,
        expressions: Vec<String>,
        depth: u32,
    },
    /// Seleciona frame para inspeção (context_get -d depth).
    SelectFrame(u32),
    /// Encerra a engine (troca de projeto / fechamento da IDE).
    Shutdown,
}

/// Fase da sessão para interrupção cirúrgica de Detach/Stop.
///
/// O shutdown do socket (para desbloquear a leitura de um `run` longo) só é
/// aplicado quando a engine está *bloqueada em `run`/`step`*. Com a sessão
/// parada no breakpoint, o socket precisa continuar vivo para o comando
/// `stop`/`detach` protocolar chegar ao Xdebug — senão o envio morre em
/// EPIPE e fechar o socket tem semântica de *detach* (o script segue
/// rodando), não de *stop* (achado com o Xdebug 3 real).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Accepting,
    Stopped,
    Continuing,
}

#[derive(Debug, Default)]
struct SessionState {
    stream: Option<TcpStream>,
    phase: Phase,
}

/// Engine de debug em thread própria.
pub struct DebugEngine {
    events: Receiver<EngineEvent>,
    commands: Sender<EngineCommand>,
    state: Arc<Mutex<SessionState>>,
    handle: Option<JoinHandle<()>>,
}

impl DebugEngine {
    /// Dispara a engine com listener, mappings e store compartilhado de
    /// breakpoints (a UI muta o mesmo store que a engine sincroniza).
    pub fn start(
        config: ListenerConfig,
        mappings: PathMappings,
        store: Arc<Mutex<BreakpointStore>>,
    ) -> io::Result<Self> {
        let (event_tx, event_rx) = channel::<EngineEvent>();
        let (cmd_tx, cmd_rx) = channel::<EngineCommand>();
        let state: Arc<Mutex<SessionState>> = Arc::new(Mutex::new(SessionState::default()));
        let thread_state = state.clone();
        let handle = std::thread::spawn(move || {
            engine_loop(config, mappings, store, event_tx, cmd_rx, thread_state);
        });
        Ok(DebugEngine {
            events: event_rx,
            commands: cmd_tx,
            state,
            handle: Some(handle),
        })
    }

    /// Drena os eventos pendentes (chamar a cada frame).
    pub fn poll(&mut self) -> Vec<EngineEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.events.try_recv() {
            out.push(ev);
        }
        out
    }

    /// Aguarda o próximo evento com prazo (para testes).
    pub fn wait_event(&mut self, timeout: Duration) -> Option<EngineEvent> {
        self.events.recv_timeout(timeout).ok()
    }

    pub fn send(&self, cmd: EngineCommand) {
        // Detach/Stop/Shutdown durante um run bloqueado: primeiro enfileira
        // o comando, DEPOIS fecha o socket — assim, quando a leitura
        // bloqueada despertar com erro, o comando já está na fila e a
        // sessão termina como Detached/Stopped (cleanup confiável). O
        // shutdown só é aplicado na fase Continuing: com a sessão parada o
        // comando é processado pelo loop com o socket vivo (o closer cego
        // quebrava o `stop` protocolar — EPIPE no envio, achado com o
        // Xdebug real: socket fechado ≈ detach, não stop).
        let interrupts = matches!(
            cmd,
            EngineCommand::Detach | EngineCommand::Stop | EngineCommand::Shutdown
        );
        let _ = self.commands.send(cmd);
        if interrupts {
            let guard = self.state.lock().unwrap();
            if let (Phase::Continuing, Some(stream)) = (guard.phase, guard.stream.as_ref()) {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
        }
    }

    /// Encerra a engine: interrompe o socket e faz join da thread.
    pub fn shutdown(&mut self) {
        self.send(EngineCommand::Shutdown);
        if let Some(stream) = self
            .state
            .lock()
            .ok()
            .as_deref()
            .and_then(|s| s.stream.as_ref())
        {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for DebugEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn engine_loop(
    config: ListenerConfig,
    mappings: PathMappings,
    store: Arc<Mutex<BreakpointStore>>,
    events: Sender<EngineEvent>,
    commands: Receiver<EngineCommand>,
    state: Arc<Mutex<SessionState>>,
) {
    let listener = match DebugListener::bind(config) {
        Ok(l) => l,
        Err(e) => {
            let _ = events.send(EngineEvent::Error(format!("listener: {e}")));
            return;
        }
    };
    let _ = events.send(EngineEvent::Listening {
        port: listener.port(),
    });

    'accept: loop {
        // Fase de aceite (o accept tem timeout interno e devolve Timeout).
        let outcome = match listener.accept(Duration::from_millis(150)) {
            Ok(outcome) => outcome,
            Err(e) => {
                let _ = events.send(EngineEvent::Error(format!("accept: {e}")));
                return;
            }
        };
        match outcome {
            AcceptOutcome::Timeout => {
                if let Some(EngineCommand::Shutdown) = drain_command(&commands) {
                    break 'accept;
                }
                continue 'accept;
            }
            AcceptOutcome::RejectedKey { got } => {
                let _ = events.send(EngineEvent::Error(format!(
                    "conexão DBGp recusada (IDE key inesperada: {:?})",
                    got
                )));
                continue 'accept;
            }
            AcceptOutcome::Accepted(mut session, init) => {
                let exit = run_session(
                    &mut session,
                    &init,
                    &mappings,
                    &store,
                    &events,
                    &commands,
                    &state,
                );
                // Sessão encerrada: volta a aceitar com o socket liberado.
                *state.lock().unwrap() = SessionState::default();
                if matches!(exit, SessionExit::Shutdown) {
                    break 'accept;
                }
                continue 'accept;
            }
        }
    }
}

/// Como o loop de sessão terminou.
enum SessionExit {
    /// Comando Shutdown recebido: encerrar a engine inteira.
    Shutdown,
    /// Sessão terminou (fim/detach/stop); volta a aceitar.
    Done,
}

#[allow(clippy::too_many_arguments)]
fn run_session(
    session: &mut DbgpSession,
    init: &InitInfo,
    mappings: &PathMappings,
    store: &Arc<Mutex<BreakpointStore>>,
    events: &Sender<EngineEvent>,
    commands: &Receiver<EngineCommand>,
    state: &Arc<Mutex<SessionState>>,
) -> SessionExit {
    let matched = mappings.classify(&init.fileuri);
    // Socket registrado para interrupção de Detach/Stop — o shutdown externo
    // só é aplicado na fase Continuing (send()).
    if let Ok(raw) = session.raw_stream().try_clone() {
        let mut guard = state.lock().unwrap();
        guard.stream = Some(raw);
        guard.phase = Phase::Stopped;
    }
    // Leitura bloqueante: interrupção de Detach/Stop vem do shutdown do
    // closer (timeout durante um run longo seria falso ConnectionLost).
    let _ = session.set_read_timeout(None);

    // Sincroniza breakpoints antes do primeiro run (store compartilhado).
    let synced = {
        let mut target = Some(&mut *session);
        store.lock().unwrap().sync(&mut target, mappings)
    };
    let _ = events.send(EngineEvent::Connected {
        init: init.clone(),
        matched,
        synced,
    });

    // Auto-resume após conectar (session_start=none padrão do Xdebug).
    let _ = events.send(EngineEvent::Continued(ContinueCommand::Resume));
    let to_local = |uri: &str| mappings.to_local(uri);
    state.lock().unwrap().phase = Phase::Continuing;
    let initial_outcome = session.continue_exec(ContinueCommand::Resume, &to_local);
    state.lock().unwrap().phase = Phase::Stopped;
    match initial_outcome {
        Ok(crate::session::StepOutcome::Stopped(reason)) => {
            let frames = session.stack(&to_local).unwrap_or_default();
            // Variáveis do frame 0 já vêm prontas para a UI.
            let variables = session.variables(0).unwrap_or_default();
            let _ = events.send(EngineEvent::Stopped { reason, frames });
            let _ = events.send(EngineEvent::FrameVariables(variables));
        }
        Ok(crate::session::StepOutcome::Ended) => {
            let _ = events.send(EngineEvent::Ended(SessionEnd::Finished));
            return SessionExit::Done;
        }
        Ok(crate::session::StepOutcome::Disconnected) | Err(_) => {
            // Pode ser interrupção legítima (Detach/Stop via closer) — o
            // comando já está na fila; classifica o fim da sessão.
            return match commands.try_recv() {
                Ok(EngineCommand::Detach) => {
                    session.close();
                    let _ = events.send(EngineEvent::Ended(SessionEnd::Detached));
                    SessionExit::Done
                }
                Ok(EngineCommand::Stop) => {
                    session.close();
                    let _ = events.send(EngineEvent::Ended(SessionEnd::Stopped));
                    SessionExit::Done
                }
                Ok(EngineCommand::Shutdown) => {
                    session.close();
                    let _ = events.send(EngineEvent::Ended(SessionEnd::Stopped));
                    SessionExit::Shutdown
                }
                other => {
                    let detail = other
                        .map(|c| format!("{c:?}"))
                        .unwrap_or_else(|_| "nenhum".into());
                    let _ = events.send(EngineEvent::Ended(SessionEnd::ConnectionLost(format!(
                        "socket do Xdebug fechado no run inicial (fila: {detail})"
                    ))));
                    SessionExit::Done
                }
            };
        }
    }

    // Loop de comandos: a sessão está parada.
    loop {
        let cmd = match commands.recv_timeout(Duration::from_millis(120)) {
            Ok(cmd) => cmd,
            Err(_) => continue,
        };
        match cmd {
            EngineCommand::UpdateBreakpoints => {
                let synced = {
                    let mut target = Some(&mut *session);
                    store.lock().unwrap().sync(&mut target, mappings)
                };
                let _ = events.send(EngineEvent::BreakpointsSynced(synced));
            }
            EngineCommand::Evaluate { expression, depth } => {
                let result = session
                    .evaluate(&expression, depth)
                    .map_err(|e| e.to_string());
                let _ = events.send(EngineEvent::Evaluated { expression, result });
            }
            EngineCommand::EvaluateWatches {
                ids,
                expressions,
                depth,
            } => {
                let watches: Vec<crate::session::Watch> = ids
                    .into_iter()
                    .zip(expressions)
                    .map(|(id, expression)| crate::session::Watch { id, expression })
                    .collect();
                let _ = events.send(EngineEvent::WatchValues(
                    session.evaluate_watches(&watches, depth),
                ));
            }
            EngineCommand::SelectFrame(depth) => {
                let variables = session.variables(depth).unwrap_or_default();
                let _ = events.send(EngineEvent::FrameVariables(variables));
            }
            EngineCommand::Continue(cont) => {
                let _ = events.send(EngineEvent::Continued(cont));
                state.lock().unwrap().phase = Phase::Continuing;
                let outcome = session.continue_exec(cont, &to_local);
                state.lock().unwrap().phase = Phase::Stopped;
                match outcome {
                    Ok(crate::session::StepOutcome::Stopped(reason)) => {
                        if let StopReason::Location { file, line } = &reason {
                            store.lock().unwrap().hit_at(file, *line);
                        }
                        let frames = session.stack(&to_local).unwrap_or_default();
                        let variables = session.variables(0).unwrap_or_default();
                        let _ = events.send(EngineEvent::Stopped { reason, frames });
                        let _ = events.send(EngineEvent::FrameVariables(variables));
                    }
                    Ok(crate::session::StepOutcome::Ended) => {
                        let _ = events.send(EngineEvent::Ended(SessionEnd::Finished));
                        return SessionExit::Done;
                    }
                    Ok(crate::session::StepOutcome::Disconnected) | Err(_) => {
                        // Interrupção (Detach/Stop via closer) ou morte do socket.
                        match commands.try_recv() {
                            Ok(EngineCommand::Detach) => {
                                session.close();
                                let _ = events.send(EngineEvent::Ended(SessionEnd::Detached));
                            }
                            Ok(EngineCommand::Stop) => {
                                session.close();
                                let _ = events.send(EngineEvent::Ended(SessionEnd::Stopped));
                            }
                            Ok(EngineCommand::Shutdown) => {
                                session.close();
                                let _ = events.send(EngineEvent::Ended(SessionEnd::Stopped));
                                return SessionExit::Shutdown;
                            }
                            other => {
                                let detail = other
                                    .map(|c| format!("{c:?}"))
                                    .unwrap_or_else(|_| "nenhum".into());
                                let _ =
                                    events.send(EngineEvent::Ended(SessionEnd::ConnectionLost(
                                        format!("socket do Xdebug fechado ({detail})"),
                                    )));
                            }
                        }
                        return SessionExit::Done;
                    }
                }
            }
            EngineCommand::Detach => {
                let _ = session.detach();
                let _ = events.send(EngineEvent::Ended(SessionEnd::Detached));
                return SessionExit::Done;
            }
            EngineCommand::Stop => {
                let _ = session.stop();
                let _ = events.send(EngineEvent::Ended(SessionEnd::Stopped));
                return SessionExit::Done;
            }
            EngineCommand::Shutdown => {
                session.close();
                let _ = events.send(EngineEvent::Ended(SessionEnd::Stopped));
                return SessionExit::Shutdown;
            }
        }
    }
}

fn drain_command(commands: &Receiver<EngineCommand>) -> Option<EngineCommand> {
    commands.try_recv().ok()
}
