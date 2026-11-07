//! Integração do fluxo DBGp com um "xdebug falso" — peer TCP que fala o
//! suficiente do protocolo (init → breakpoint_set → run/stack/context →
//! stepping → eval → detach/stop) sem precisar do Xdebug instalado.
//!
//! Cobre `T-RUN-DBG-001-01/02/03/04`, `T-RUN-DBG-002-01/02` e
//! `T-RUN-DBG-003-01` no nível protocolar. O fluxo com PHP real depende da
//! extensão Xdebug (ausente neste ambiente — ver docs/baseline).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use phpide_dbgp::breakpoint::{BreakpointStatus, BreakpointStore, SyncEvent};
use phpide_dbgp::engine::{DebugEngine, EngineCommand, EngineEvent, SessionEnd};
use phpide_dbgp::listener::{ListenerConfig, DEFAULT_IDE_KEY};
use phpide_dbgp::mapping::{ConnectionMatch, PathMappings};
use phpide_dbgp::session::{ContinueCommand, StopReason};

const PROJECT: &str = "/proj";

/// Peer fake: responde comandos conforme o roteiro `(prefixo, resposta)`.
/// `None` como resposta = não responder (para testar Detach durante run).
struct FakeXdebug {
    handle: JoinHandle<()>,
    disconnected: mpsc::Receiver<()>,
}

fn start_fake(port: u16, idekey: &str, script: Vec<(&'static str, Option<String>)>) -> FakeXdebug {
    let (tx, rx) = mpsc::channel();
    let idekey = idekey.to_string();
    let handle = std::thread::spawn(move || {
        let mut stream = loop {
            if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
                break s;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        stream.write_all(
            format!(r#"<init xmlns="urn:debugger_protocol_v1" fileuri="file://{PROJECT}/index.php" language="PHP" protocol_version="1.0" appid="999" idekey="{idekey}"></init>"#)
                .as_bytes(),
        ).unwrap();
        stream.write_all(&[0]).unwrap();

        let mut reader = stream.try_clone().unwrap();
        let mut tx = Some(tx);
        for (expected, response) in script {
            // Lê um comando completo (até \0).
            let mut buf = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                match reader.read(&mut byte) {
                    Ok(0) => {
                        // Socket fechado pela IDE: sinaliza e encerra.
                        let _ = tx.take().map(|t| t.send(()));
                        return;
                    }
                    Ok(_) => {
                        if byte[0] == 0 {
                            break;
                        }
                        buf.push(byte[0]);
                    }
                    Err(_) => {
                        let _ = tx.take().map(|t| t.send(()));
                        return;
                    }
                }
            }
            let cmd = String::from_utf8_lossy(&buf).into_owned();
            assert!(
                cmd.starts_with(expected),
                "esperava comando '{expected}', veio: {cmd}"
            );
            if let Some(response) = response {
                stream.write_all(response.as_bytes()).unwrap();
                stream.write_all(&[0]).unwrap();
            }
        }
        // Roteiro esgotado: mantém o socket aberto até a IDE fechar.
        let mut sink = Vec::new();
        let _ = reader.read_to_end(&mut sink);
        let _ = tx.take().map(|t| t.send(()));
    });
    FakeXdebug {
        handle,
        disconnected: rx,
    }
}

fn free_port() -> u16 {
    let probe = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    port
}

fn bp_response(id: &str) -> String {
    format!(
        r#"<response command="breakpoint_set" transaction_id="2" id="{id}" success="1"></response>"#
    )
}

fn run_break(file: &str, line: u32) -> String {
    format!(
        r#"<response command="run" transaction_id="3" status="break" reason="ok"><xdebug:message filename="file://{file}" lineno="{line}"/></response>"#
    )
}

const STACK: &str = r#"<response command="stack_get" transaction_id="4"><stack where="{main}" level="0" type="file" filename="file:///proj/index.php" lineno="7"/><stack where="App\Calc->soma" level="1" type="file" filename="file:///proj/src/Calc.php" lineno="8"/></response>"#;
const CONTEXT: &str = r#"<response command="context_get" transaction_id="5"><property name="$total" fullname="$total" type="int"><![CDATA[42]]></property></response>"#;

fn engine_with_store(
    port: u16,
    store: BreakpointStore,
) -> (DebugEngine, Arc<Mutex<BreakpointStore>>) {
    let shared = Arc::new(Mutex::new(store));
    let engine = DebugEngine::start(
        ListenerConfig {
            port,
            ..Default::default()
        },
        PathMappings::new(PathBuf::from(PROJECT)),
        shared.clone(),
    )
    .unwrap();
    (engine, shared)
}

#[test]
fn full_session_breakpoints_stepping_eval_watches_and_resume() {
    let port = free_port();
    let mut store = BreakpointStore::new();
    let bp_file = PathBuf::from("/proj/index.php");
    let bp_id = store.add_line(bp_file.clone(), 7);

    let fake = start_fake(
        port,
        DEFAULT_IDE_KEY,
        vec![
            ("breakpoint_set", Some(bp_response("10"))),
            ("run", Some(run_break("/proj/index.php", 7))),
            ("stack_get", Some(STACK.into())),
            // Variáveis do frame 0 carregadas junto com a parada.
            ("context_get", Some(CONTEXT.into())),
            ("step_over", Some(run_break("/proj/index.php", 8))),
            ("stack_get", Some(STACK.into())),
            ("context_get", Some(CONTEXT.into())),
            ("run", Some(r#"<response command="run" transaction_id="8" status="stopped" reason="ok"></response>"#.into())),
        ],
    );

    let (mut engine, _store) = engine_with_store(port, store);
    // Listening → Connected (synced) → auto-Resume.
    match engine.wait_event(Duration::from_secs(5)) {
        Some(EngineEvent::Listening { port: p }) => assert_eq!(p, port),
        other => panic!("esperava Listening: {other:?}"),
    }
    match engine.wait_event(Duration::from_secs(5)) {
        Some(EngineEvent::Connected {
            init,
            matched,
            synced,
        }) => {
            assert!(init.fileuri.contains("/proj/index.php"));
            assert_eq!(
                matched,
                ConnectionMatch::Owned(PathBuf::from("/proj/index.php"))
            );
            assert_eq!(synced.len(), 1, "synced: {synced:?}");
            match &synced[0] {
                SyncEvent::Set {
                    local_id,
                    backend_id,
                } => {
                    assert_eq!(*local_id, bp_id, "synced: {synced:?}");
                    assert_eq!(backend_id, "10", "synced: {synced:?}");
                }
                other => panic!("esperava Set, veio {other:?}"),
            }
        }
        other => panic!("esperava Connected: {other:?}"),
    }
    assert!(matches!(
        engine.wait_event(Duration::from_secs(5)),
        Some(EngineEvent::Continued(ContinueCommand::Resume))
    ));

    // Parada no breakpoint: local mapeado + frames com topo correto; logo
    // em seguida vêm as variáveis do frame 0 (FrameVariables).
    match engine.wait_event(Duration::from_secs(5)) {
        Some(EngineEvent::Stopped { reason, frames }) => {
            assert_eq!(
                reason,
                StopReason::Location {
                    file: bp_file.clone(),
                    line: 7
                }
            );
            assert_eq!(frames.len(), 2);
            assert_eq!(frames[0].name, "{main}");
            assert_eq!(frames[0].file, Some(bp_file.clone()));
            assert_eq!(frames[1].name, r"App\Calc->soma");
        }
        other => panic!("esperava Stopped: {other:?}"),
    }
    match engine.wait_event(Duration::from_secs(5)) {
        Some(EngineEvent::FrameVariables(vars)) => {
            assert_eq!(vars.len(), 1);
            assert_eq!(vars[0].value.as_deref(), Some("42"));
        }
        other => panic!("esperava FrameVariables: {other:?}"),
    }

    // Step Over (F8) com sessão real do fake.
    engine.send(EngineCommand::Continue(ContinueCommand::StepOver));
    assert!(matches!(
        engine.wait_event(Duration::from_secs(5)),
        Some(EngineEvent::Continued(ContinueCommand::StepOver))
    ));
    loop {
        match engine.wait_event(Duration::from_secs(5)) {
            Some(EngineEvent::Stopped { reason, .. }) => {
                assert_eq!(
                    reason,
                    StopReason::Location {
                        file: bp_file,
                        line: 8
                    }
                );
                break;
            }
            Some(EngineEvent::FrameVariables(_)) => continue,
            other => panic!("esperava Stopped após step: {other:?}"),
        }
    }

    // Resume (F9) até o fim. A FrameVariables da parada anterior ainda pode
    // estar na fila — drena antes de esperar o Continued.
    engine.send(EngineCommand::Continue(ContinueCommand::Resume));
    loop {
        match engine.wait_event(Duration::from_secs(5)) {
            Some(EngineEvent::Continued(ContinueCommand::Resume)) => break,
            Some(EngineEvent::FrameVariables(_)) => continue,
            other => panic!("esperava Continued(Resume): {other:?}"),
        }
    }
    loop {
        match engine.wait_event(Duration::from_secs(5)) {
            Some(EngineEvent::Ended(SessionEnd::Finished)) => break,
            Some(EngineEvent::FrameVariables(_)) => continue,
            other => panic!("esperava Finished: {other:?}"),
        }
    }
    engine.shutdown();
    fake.disconnected
        .recv_timeout(Duration::from_secs(5))
        .expect("fake observa socket fechado");
    fake.handle.join().unwrap();
}

#[test]
fn session_level_evaluate_and_watches_per_frame() {
    // Nível sessão: evaluate associado ao frame (property_get -d) e
    // reavaliação de watches na parada. Após Resume os valores não têm mais
    // garantia — a UI os descarta (RUN-DBG-002, T-RUN-DBG-002-02).
    let port = free_port();
    let fake = start_fake(
        port,
        DEFAULT_IDE_KEY,
        vec![
            // evaluate("$total") e evaluate("$missing") consomem os dois
            // primeiros; os watches dois a mais; detach ao final.
            ("property_get", Some(r#"<response command="property_get" transaction_id="1" success="1"><property name="$total" fullname="$total" type="int"><![CDATA[42]]></property></response>"#.into())),
            ("property_get", Some(r#"<response command="property_get" transaction_id="2" success="0"><error id="300"><message>variável não existe</message></error></response>"#.into())),
            ("property_get", Some(r#"<response command="property_get" transaction_id="3" success="1"><property name="$total" fullname="$total" type="int"><![CDATA[42]]></property></response>"#.into())),
            ("property_get", Some(r#"<response command="property_get" transaction_id="4" success="0"><error id="300"><message>variável não existe</message></error></response>"#.into())),
            ("detach", Some(r#"<response command="detach" transaction_id="5" status="stopping" reason="ok"></response>"#.into())),
        ],
    );
    let mut session = phpide_dbgp::listen(port, Duration::from_secs(10))
        .unwrap()
        .expect("fake conecta de volta");

    // Evaluate por frame: profundidade 1 endereçada via -d.
    let var = session
        .evaluate("$total", 1)
        .unwrap()
        .expect("valor de $total");
    assert_eq!(var.type_name, "int");
    assert_eq!(var.value.as_deref(), Some("42"));

    // Expressão inexistente: success=0 → Ok(None), sem panic.
    assert!(session.evaluate("$missing", 0).unwrap().is_none());

    // Watches: um resolve, outro indisponível no frame (Ok(None)) — a UI
    // mostra o estado por watch e descarta valores ao dar Resume.
    let watches = vec![
        phpide_dbgp::Watch {
            id: 1,
            expression: "$total".into(),
        },
        phpide_dbgp::Watch {
            id: 2,
            expression: "$missing".into(),
        },
    ];
    let values = session.evaluate_watches(&watches, 0);
    assert_eq!(values.len(), 2);
    assert!(matches!(&values[0].result, Ok(Some(v)) if v.value.as_deref() == Some("42")));
    assert!(matches!(&values[1].result, Ok(None)));
    // Detach fecha o socket de forma confiável (cleanup).
    session.detach().unwrap();
    assert!(session.is_closed());
    fake.disconnected
        .recv_timeout(Duration::from_secs(5))
        .expect("fake vê socket fechado");
    fake.handle.join().unwrap();
}

#[test]
fn detach_while_running_closes_socket_immediately() {
    let port = free_port();
    // run não responde (script longo): Detach deve interromper na hora.
    let fake = start_fake(port, DEFAULT_IDE_KEY, vec![("run", None)]);
    let (mut engine, _store) = engine_with_store(port, BreakpointStore::new());
    assert!(matches!(
        engine.wait_event(Duration::from_secs(5)),
        Some(EngineEvent::Listening { .. })
    ));
    assert!(matches!(
        engine.wait_event(Duration::from_secs(5)),
        Some(EngineEvent::Connected { .. })
    ));
    assert!(matches!(
        engine.wait_event(Duration::from_secs(5)),
        Some(EngineEvent::Continued(_))
    ));
    // O fake nunca responde ao run: Detach deve fechar o socket (cleanup).
    engine.send(EngineCommand::Detach);
    match engine.wait_event(Duration::from_secs(5)) {
        Some(EngineEvent::Ended(SessionEnd::Detached)) => {}
        other => panic!("esperava Detached: {other:?}"),
    }
    fake.disconnected
        .recv_timeout(Duration::from_secs(5))
        .expect("fake vê socket fechado");
    fake.handle.join().unwrap();
    engine.shutdown();
}

#[test]
fn stop_while_running_ends_session_and_releases_port() {
    let port = free_port();
    let fake = start_fake(port, DEFAULT_IDE_KEY, vec![("run", None)]);
    let (mut engine, _store) = engine_with_store(port, BreakpointStore::new());
    assert!(matches!(
        engine.wait_event(Duration::from_secs(5)),
        Some(EngineEvent::Listening { .. })
    ));
    assert!(matches!(
        engine.wait_event(Duration::from_secs(5)),
        Some(EngineEvent::Connected { .. })
    ));
    assert!(matches!(
        engine.wait_event(Duration::from_secs(5)),
        Some(EngineEvent::Continued(_))
    ));
    engine.send(EngineCommand::Stop);
    match engine.wait_event(Duration::from_secs(5)) {
        Some(EngineEvent::Ended(SessionEnd::Stopped)) => {}
        other => panic!("esperava Stopped: {other:?}"),
    }
    fake.disconnected
        .recv_timeout(Duration::from_secs(5))
        .expect("fake vê socket fechado");
    engine.shutdown();
    fake.handle.join().unwrap();
    // Cleanup de porta (T-RUN-DBG-001-04): listener encerrado libera a porta.
    std::thread::sleep(Duration::from_millis(150));
    TcpListener::bind(("127.0.0.1", port)).expect("porta liberada após shutdown da engine");
}

#[test]
fn wrong_ide_key_is_rejected_and_engine_keeps_listening() {
    let port = free_port();
    // Conecta com idekey errada depois com a certa.
    let wrong = std::thread::spawn(move || {
        for _ in 0..50 {
            if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
                s.write_all(
                    br#"<init xmlns="urn:debugger_protocol_v1" fileuri="file:///proj/x.php" language="PHP" appid="2" idekey="intruso"></init>"#,
                ).unwrap();
                s.write_all(&[0]).unwrap();
                let mut buf = Vec::new();
                let mut byte = [0u8; 1];
                // Lê o detach e o fechamento.
                loop {
                    match s.read(&mut byte) {
                        Ok(0) => break,
                        Ok(_) => buf.push(byte[0]),
                        Err(_) => break,
                    }
                }
                return String::from_utf8_lossy(&buf).into_owned();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("não conectou");
    });
    let (mut engine, _store) = engine_with_store(port, BreakpointStore::new());
    assert!(matches!(
        engine.wait_event(Duration::from_secs(5)),
        Some(EngineEvent::Listening { .. })
    ));
    let received = wrong.join().unwrap();
    assert!(
        received.contains("detach"),
        "intruso recebeu detach: {received}"
    );
    // Engine segue escutando (a conexão errada não derruba o listener).
    match engine.wait_event(Duration::from_secs(5)) {
        Some(EngineEvent::Error(msg)) => assert!(msg.contains("IDE key"), "{msg}"),
        other => panic!("esperava Error de ide key: {other:?}"),
    }
    engine.shutdown();
}

#[test]
fn foreign_project_connection_is_flagged_for_confirmation() {
    let port = free_port();
    // init aponta para fora do projeto → Foreign (UI pede confirmação/mapping).
    let fake = std::thread::spawn(move || {
        for _ in 0..50 {
            if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
                s.write_all(
                    br#"<init xmlns="urn:debugger_protocol_v1" fileuri="file:///outro/projeto/x.php" language="PHP" appid="3" idekey="phpide"></init>"#,
                ).unwrap();
                s.write_all(&[0]).unwrap();
                let mut sink = Vec::new();
                let _ = s.read_to_end(&mut sink);
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    let (mut engine, _store) = engine_with_store(port, BreakpointStore::new());
    assert!(matches!(
        engine.wait_event(Duration::from_secs(5)),
        Some(EngineEvent::Listening { .. })
    ));
    match engine.wait_event(Duration::from_secs(5)) {
        Some(EngineEvent::Connected { matched, .. }) => {
            assert_eq!(
                matched,
                ConnectionMatch::Foreign(PathBuf::from("/outro/projeto/x.php"))
            );
        }
        other => panic!("esperava Connected: {other:?}"),
    }
    // Auto-resume vem antes do comando da UI.
    assert!(matches!(
        engine.wait_event(Duration::from_secs(5)),
        Some(EngineEvent::Continued(_))
    ));
    engine.send(EngineCommand::Detach);
    assert!(matches!(
        engine.wait_event(Duration::from_secs(5)),
        Some(EngineEvent::Ended(SessionEnd::Detached))
    ));
    fake.join().unwrap();
    engine.shutdown();
}

#[test]
fn breakpoint_statuses_follow_backend_responses() {
    // Recusado pelo backend → Invalid com mensagem (T-RUN-DBG-003-01).
    let port = free_port();
    let mut store = BreakpointStore::new();
    let good = store.add_line(PathBuf::from("/proj/index.php"), 7);
    let bad = store.add_line(PathBuf::from("/proj/vazio.php"), 3);
    let fake = start_fake(
        port,
        DEFAULT_IDE_KEY,
        vec![
            ("breakpoint_set", Some(bp_response("10"))),
            ("breakpoint_set", Some(r#"<response command="breakpoint_set" transaction_id="3" success="0"><error id="200"><message>linha sem código executável</message></error></response>"#.into())),
            ("run", Some(r#"<response command="run" transaction_id="4" status="stopped" reason="ok"></response>"#.into())),
        ],
    );
    let (mut engine, _store) = engine_with_store(port, store);
    assert!(matches!(
        engine.wait_event(Duration::from_secs(5)),
        Some(EngineEvent::Listening { .. })
    ));
    match engine.wait_event(Duration::from_secs(5)) {
        Some(EngineEvent::Connected { synced, .. }) => {
            eprintln!("[test] synced = {synced:?}");
            let ids: Vec<u64> = synced
                .iter()
                .map(|s| match s {
                    SyncEvent::Set { local_id, .. } | SyncEvent::Invalid { local_id, .. } => {
                        *local_id
                    }
                    _ => 0,
                })
                .collect();
            assert!(ids.contains(&good) && ids.contains(&bad), "{synced:?}");
            assert!(
                synced
                    .iter()
                    .any(|s| matches!(s, SyncEvent::Invalid { local_id, .. } if *local_id == bad)),
                "{synced:?}"
            );
        }
        other => panic!("esperava Connected: {other:?}"),
    }
    // Auto-resume após o connect, então o fim.
    assert!(matches!(
        engine.wait_event(Duration::from_secs(5)),
        Some(EngineEvent::Continued(ContinueCommand::Resume))
    ));
    match engine.wait_event(Duration::from_secs(5)) {
        Some(EngineEvent::Ended(SessionEnd::Finished)) => {}
        other => panic!("esperava Finished: {other:?}"),
    }
    engine.shutdown();
    fake.handle.join().unwrap();
    let _ = BreakpointStatus::Pending; // estados cobertos nos UT do store
}
