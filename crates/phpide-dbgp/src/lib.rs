//! phpide-dbgp — cliente DBGp/Xdebug (Estágio 4 → Estágio 8).
//!
//! Protocolo de depuração do Xdebug: XML sobre TCP, mensagens terminadas em
//! `\0`. A IDE escuta a porta, o PHP (com Xdebug) conecta de volta, breakpoints
//! são sincronizados e a execução é controlada por comandos DBGp. Painéis e
//! controles `egui` ficam na UI — aqui é só protocolo e máquina de estados.
//!
//! - [`proto`]: codificação/parsing de pacotes e respostas;
//! - [`breakpoint`]: modelo e estados dos breakpoints + sincronização
//!   (T-RUN-DBG-003-01);
//! - [`mapping`]: path mappings local↔remoto e associação da conexão ao
//!   projeto (T-RUN-DBG-001-02);
//! - [`listener`]: listener configurável, IDE key e XDEBUG_CONFIG
//!   (T-RUN-DBG-001-01);
//! - [`session`]: run/stepping, frames, variáveis, evaluate e watches
//!   (T-RUN-DBG-001-03, T-RUN-DBG-002-01/02);
//! - [`engine`]: máquina de estados da sessão com cleanup confiável de
//!   sockets e processos (T-RUN-DBG-001-03/04).

pub mod breakpoint;
pub mod engine;
pub mod listener;
pub mod mapping;
pub mod proto;
pub mod session;

pub use breakpoint::{Breakpoint, BreakpointKind, BreakpointStatus, BreakpointStore, SyncEvent};
pub use engine::{DebugEngine, EngineCommand, EngineEvent, SessionEnd};
pub use listener::{
    parse_init, xdebug_env, AcceptOutcome, DebugListener, InitInfo, ListenerConfig, DEFAULT_IDE_KEY,
};
pub use mapping::{ConnectionMatch, PathMappings};
pub use proto::{
    attr, encode_command, error_message, fileuri, listen, parse_response, property_names,
    read_packet, uri_to_path, DbgpSession, ResponseInfo, DEFAULT_PORT,
};
pub use session::{
    base64_encode, parse_context, parse_stack, ContinueCommand, Frame, StepOutcome, StopReason,
    Variable, Watch, WatchValue,
};

/// Mensagens para o painel Execução durante uma sessão de debug (legado
/// Estágio 4, usado pela UI até a migração para a engine).
pub enum DebugMsg {
    Line(String),
    Done,
}

/// Sessão completa do MVP (Estágio 4): breakpoints -> run -> (pilha+variáveis
/// se parar) -> detach. Mantida para compatibilidade do painel legado; o
/// fluxo novo usa [`DebugEngine`].
pub fn run_with_breakpoints(
    port: u16,
    wait: std::time::Duration,
    breakpoints: &[(std::path::PathBuf, u32)],
    tx: std::sync::mpsc::Sender<DebugMsg>,
) -> std::io::Result<()> {
    let say = |tx: &std::sync::mpsc::Sender<DebugMsg>, line: String| {
        let _ = tx.send(DebugMsg::Line(line));
    };
    say(
        &tx,
        format!("[debug] aguardando xdebug em 127.0.0.1:{port} ({wait:?})..."),
    );
    let Some(mut session) = listen(port, wait)? else {
        say(
            &tx,
            "[debug] tempo esgotado — xdebug não conectou".to_string(),
        );
        let _ = tx.send(DebugMsg::Done);
        return Ok(());
    };
    say(&tx, "[debug] xdebug conectado".to_string());

    for (path, line) in breakpoints {
        let uri = fileuri(path);
        let ln = line.to_string();
        match session.send("breakpoint_set", &[("t", "line"), ("f", &uri), ("n", &ln)]) {
            Ok(r) if r.success => say(&tx, format!("[debug] breakpoint {uri}:{ln} ok")),
            Ok(_) => say(&tx, format!("[debug] breakpoint {uri}:{ln} recusado")),
            Err(e) => say(&tx, format!("[debug] erro no breakpoint: {e}")),
        }
    }

    match session.send("run", &[]) {
        Ok(r) => {
            say(&tx, format!("[debug] run -> status={}", r.status));
            if r.status == "break" {
                if let Ok(stack) = session.send_xml("stack_get", &[]) {
                    let frame = stack
                        .match_indices("<stack ")
                        .next()
                        .and_then(|(i, _)| {
                            let f = attr(&stack[i..], "filename")?;
                            let l = attr(&stack[i..], "lineno")?;
                            Some(format!("{f}:{}", l))
                        })
                        .unwrap_or_else(|| "?".to_string());
                    say(&tx, format!("[debug] parado em {frame}"));
                }
                if let Ok(ctx) = session.send_xml("context_get", &[("c", "0")]) {
                    let names = property_names(&ctx);
                    say(
                        &tx,
                        format!("[debug] variáveis locais: {}", names.join(", ")),
                    );
                }
            }
        }
        Err(e) => say(&tx, format!("[debug] erro no run: {e}")),
    }

    let _ = session.send("detach", &[]);
    session.close();
    say(&tx, "[debug] sessão encerrada (detach)".to_string());
    let _ = tx.send(DebugMsg::Done);
    Ok(())
}
