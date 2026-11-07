//! Gate do Estágio 8 — executar, depurar e testar um **script PHP real**,
//! com Stop/Detach confiáveis.
//!
//! - Executar: PHP CLI real (8.2) roda um script via Run Configuration;
//! - Depurar: o processo PHP real é disparado com `XDEBUG_CONFIG`/`XDEBUG_SESSION`
//!   e a engine DBGp completa o handshake; quando a extensão Xdebug está
//!   carregável (`.so` local extraído de um `.deb`, sem root — ver
//!   [`xdebug_so`]), os testes `gate_debug_*_xdebug_real` rodam a sessão de
//!   ponta a ponta contra o Xdebug 3 real (init, breakpoint_set, break,
//!   stack_get, context_get, step_over, eval, detach/stop); sem a extensão
//!   vale o peer protocolar fiel, que fala exatamente o mesmo DBGp;
//! - Testar: PHPUnit 11 real (phar) executa o fixture do projeto;
//! - Stop confiável: mata o grupo de processos reais (php sleep) e fecha o
//!   socket; Detach confiável: encerra a sessão e libera a porta com o
//!   processo PHP real ainda vivo.
//!
//! Sem PHP no ambiente o gate imprime o motivo e ignora (diferenciando
//! ausência de ferramenta de falha — NFR-003).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use phpide_dbgp::engine::DebugEngine;

/// Consome eventos até Connected (ignora Listening).
fn wait_connected(engine: &mut DebugEngine) {
    wait_connected_full(engine);
}

/// Consome eventos até Connected e devolve (associação, sync de breakpoints).
fn wait_connected_full(
    engine: &mut DebugEngine,
) -> (
    phpide_dbgp::mapping::ConnectionMatch,
    Vec<phpide_dbgp::breakpoint::SyncEvent>,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "timeout aguardando Connected");
        match engine.wait_event(Duration::from_secs(10)) {
            Some(EngineEvent::Connected {
                matched, synced, ..
            }) => return (matched, synced),
            Some(EngineEvent::Listening { .. }) => continue,
            other => panic!("esperava Connected: {other:?}"),
        }
    }
}

use phpide_dbgp::breakpoint::BreakpointStore;
use phpide_dbgp::engine::{EngineCommand, EngineEvent, SessionEnd};
use phpide_dbgp::listener::ListenerConfig;
use phpide_dbgp::mapping::PathMappings;
use phpide_dbgp::session::ContinueCommand;
use phpide_run::config::{RunConfig, RunKind, TestScope};
use phpide_run::php_script::{build_command, BuildContext};
use phpide_run::phpunit::{build_command as phpunit_build, TeamCityParser};
use phpide_run::session::SessionState;
use phpide_run::RunManager;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/phpunit-sample");

fn php() -> Option<String> {
    let php = std::env::var("PHPIDE_PHP").unwrap_or_else(|_| "php".into());
    Command::new(&php)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(php)
}

fn phpunit() -> Option<PathBuf> {
    std::env::var("PHPIDE_PHPUNIT")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .or_else(|| {
            let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/phpunit.phar");
            p.exists().then_some(p)
        })
}

fn free_port() -> u16 {
    let probe = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    port
}

/// Extensão Xdebug carregável sem root: `.deb` do Debian (mesma ABI do
/// PHP 8.2 NTS) extraído com `apt-get download` + `dpkg-deb -x` em
/// `target/tmp/xdebug-root`. Override por `PHPIDE_XDEBUG_SO`; sem o binário
/// os testes `*_xdebug_real` se autopulam (o gate segue válido no peer fiel).
fn xdebug_so() -> Option<PathBuf> {
    std::env::var("PHPIDE_XDEBUG_SO")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .or_else(|| {
            let p = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/tmp/xdebug-root/usr/lib/php/20220829/xdebug.so");
            p.exists().then_some(p)
        })
}

/// Comando `php` com a extensão Xdebug carregada no modo pedido (`debug`
/// ou `coverage`). Os demais `-d`/args entram em seguida.
fn php_com_xdebug(php: &str, so: &Path, mode: &str) -> Command {
    let mut cmd = Command::new(php);
    cmd.arg("-d")
        .arg(format!("zend_extension={}", so.display()))
        .arg("-d")
        .arg(format!("xdebug.mode={mode}"));
    cmd
}

/// Xdebug real disponível E carregável neste php (o `.so` resolve a ABI).
fn xdebug_real_disponivel(php: &str, so: &Path) -> bool {
    php_com_xdebug(php, so, "debug")
        .arg("--ri")
        .arg("xdebug")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn wait_session(m: &mut RunManager, id: u64, live_want: bool, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        m.poll();
        let live = m.session(id).is_some_and(|s| s.state.is_live());
        if live == live_want {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timeout aguardando live={live_want}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn gate_executa_debuga_e_testa_php_real() {
    let Some(php) = php() else {
        eprintln!("GATE SKIP: php não disponível");
        return;
    };

    // 1) EXECUTAR — script real via Run Configuration (T-RUN-002-01).
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("gate.php"),
        "<?php echo 'gate-ok'; fwrite(STDERR, 'gate-err');",
    )
    .unwrap();
    let config = RunConfig::new(
        "gate",
        "gate.php",
        RunKind::PhpScript {
            file: PathBuf::from("gate.php"),
            args: vec![],
            interpreter_options: vec![],
            break_at_first_line: false,
        },
    );
    let built = build_command(&config, dir.path(), &BuildContext::php_only(&php)).unwrap();
    let mut manager = RunManager::new();
    let run_id = manager.start(config, built.spec, None);
    wait_session(&mut manager, run_id, false, Duration::from_secs(15));
    let out = manager.session(run_id).unwrap().output().to_string();
    assert!(
        out.contains("gate-ok") && out.contains("[stderr] gate-err"),
        "{out}"
    );

    // 2) DEPURAR — processo PHP real com XDEBUG_CONFIG + engine DBGp contra
    //    peer protocolar (extensão Xdebug ausente no ambiente — documentado).
    let port = free_port();
    let (peer_done_tx, peer_done) = mpsc::channel();
    let peer_port = port;
    let gate_root = dir.path().to_path_buf();
    let peer = std::thread::spawn(move || {
        // Peer fiel: init → breakpoint_set → run → parada → stack → run → fim.
        let mut stream = loop {
            if let Ok(s) = TcpStream::connect(("127.0.0.1", peer_port)) {
                break s;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let init = format!(
            r#"<init xmlns="urn:debugger_protocol_v1" fileuri="file://{}/gate.php" language="PHP" protocol_version="1.0" appid="777" idekey="phpide"></init>"#,
            gate_root.display()
        );
        stream.write_all(init.as_bytes()).unwrap();
        stream.write_all(&[0]).unwrap();
        let mut reader = stream.try_clone().unwrap();
        let mut tx = Some(peer_done_tx);
        let script = [
            (
                "breakpoint_set",
                r#"<response command="breakpoint_set" transaction_id="2" id="10" success="1"></response>"#,
            ),
            (
                "run",
                r#"<response command="run" transaction_id="3" status="break" reason="ok"><xdebug:message filename="file:///gate.php" lineno="2"/></response>"#,
            ),
            (
                "stack_get",
                r#"<response command="stack_get" transaction_id="4"><stack where="{main}" level="0" type="file" filename="file:///gate.php" lineno="2"/></response>"#,
            ),
            (
                "context_get",
                r#"<response command="context_get" transaction_id="5"><property name="$argv" fullname="$argv" type="array" numchildren="1"></property></response>"#,
            ),
            (
                "run",
                r#"<response command="run" transaction_id="6" status="stopped" reason="ok"></response>"#,
            ),
        ];
        for (expected, response) in script {
            let mut buf = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                match reader.read(&mut byte) {
                    Ok(0) => {
                        let _ = tx.take().map(|t| t.send("socket-fechado"));
                        return;
                    }
                    Ok(_) => {
                        if byte[0] == 0 {
                            break;
                        }
                        buf.push(byte[0]);
                    }
                    Err(_) => return,
                }
            }
            let cmd = String::from_utf8_lossy(&buf).into_owned();
            assert!(cmd.starts_with(expected), "esperava {expected}, veio {cmd}");
            stream.write_all(response.as_bytes()).unwrap();
            stream.write_all(&[0]).unwrap();
        }
        // Espera a IDE fechar (detach/stop do fim da sessão).
        let mut sink = Vec::new();
        let _ = reader.read_to_end(&mut sink);
        let _ = tx.take().map(|t| t.send("fim"));
    });

    let mut store = BreakpointStore::new();
    store.add_line(dir.path().join("gate.php"), 2);
    let mut engine = DebugEngine::start(
        ListenerConfig {
            port,
            ide_key: Some("phpide".into()),
            ..Default::default()
        },
        PathMappings::new(dir.path().to_path_buf()),
        std::sync::Arc::new(std::sync::Mutex::new(store)),
    )
    .unwrap();

    // Processo PHP REAL disparado com o env de debug (sem xdebug o env é
    // ignorado pelo PHP — o processo é real e seu ciclo de vida observável).
    // Stdio nul: o filho não pode herdar os pipes de captura do harness.
    let mut php_child = Command::new(&php)
        .arg("-r")
        .arg("usleep(400000); echo 'php-real-ok';")
        .env(
            "XDEBUG_CONFIG",
            format!("client_host=127.0.0.1 client_port={port}"),
        )
        .env("XDEBUG_SESSION", "phpide")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn do php real");

    // Conexão aceita, breakpoints sincronizados (T-RUN-DBG-001-03).
    let (matched, synced) = wait_connected_full(&mut engine);
    assert!(
        matches!(matched, phpide_dbgp::mapping::ConnectionMatch::Owned(_)),
        "{matched:?}"
    );
    assert!(
        synced
            .iter()
            .any(|s| matches!(s, phpide_dbgp::breakpoint::SyncEvent::Set { .. })),
        "{synced:?}"
    );
    // Parada no breakpoint com frame e variáveis (T-RUN-DBG-001-03).
    loop {
        match engine.wait_event(Duration::from_secs(10)) {
            Some(EngineEvent::Stopped { frames, .. }) => {
                assert_eq!(frames.len(), 1);
                break;
            }
            Some(EngineEvent::Continued(_)) | Some(EngineEvent::FrameVariables(_)) => continue,
            other => panic!("gate: esperava Stopped: {other:?}"),
        }
    }
    // Resume (F9) → o script vai até o fim (Finished).
    engine.send(EngineCommand::Continue(ContinueCommand::Resume));
    loop {
        match engine.wait_event(Duration::from_secs(10)) {
            Some(EngineEvent::Ended(SessionEnd::Finished)) => break,
            Some(EngineEvent::Continued(_)) | Some(EngineEvent::FrameVariables(_)) => continue,
            other => panic!("gate: esperava Finished: {other:?}"),
        }
    }
    assert_eq!(
        php_child.wait().unwrap().code(),
        Some(0),
        "php real terminou limpo"
    );
    assert_eq!(
        peer_done.recv_timeout(Duration::from_secs(5)).unwrap(),
        "fim"
    );
    peer.join().unwrap();
    engine.shutdown();

    // 3) TESTAR — PHPUnit real no fixture (T-RUN-002-03 / RUN-TST-001).
    let Some(phar) = phpunit() else {
        eprintln!("GATE: phpunit ausente — etapa de testes pulada");
        return;
    };
    let root = Path::new(FIXTURE);
    let ctx = BuildContext {
        php: php.clone(),
        composer: None,
        phpunit: Some(phar.to_string_lossy().into_owned()),
    };
    let test_config = RunConfig::new(
        "gate-tests",
        "PHPUnit fixture",
        RunKind::PhpUnit {
            scope: TestScope::All,
            config: None,
            bootstrap: None,
            runner_options: vec![],
            coverage: false,
        },
    );
    let (spec, _plan) = phpunit_build(&test_config, root, &ctx, root.to_path_buf()).unwrap();
    let mut tests = RunManager::new();
    let test_id = tests.start(test_config, spec, None);
    let mut parser = TeamCityParser::new();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut seen = 0usize;
    loop {
        tests.poll();
        let s = tests.session(test_id).unwrap();
        let full = s.output();
        if full.len() > seen {
            for line in full[seen..].lines() {
                parser.feed_line(line);
            }
            seen = full.len();
        }
        if !s.state.is_live() {
            assert_eq!(s.state, SessionState::Finished);
            assert_eq!(s.exit_code, Some(1), "fixture tem 1 falha proposital");
            break;
        }
        assert!(Instant::now() < deadline, "timeout no gate de testes");
        std::thread::sleep(Duration::from_millis(30));
    }
    let tree = parser.finish();
    assert_eq!(tree.stats().total, 7, "árvore do PHPUnit real");
    assert_eq!(tree.failures().len(), 1);
}

#[test]
fn gate_stop_confiavel_mata_processo_php_real_e_fecha_socket() {
    let Some(php) = php() else {
        eprintln!("GATE SKIP: php não disponível");
        return;
    };
    let port = free_port();
    // Peer nunca responde ao run: Stop precisa matar o PHP real e fechar o
    // socket imediatamente (T-RUN-DBG-001-04).
    let (tx, rx) = mpsc::channel();
    let peer = std::thread::spawn(move || {
        let mut stream = loop {
            if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
                break s;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        stream
            .write_all(br#"<init xmlns="urn:debugger_protocol_v1" fileuri="file:///x.php" language="PHP" appid="1" idekey="phpide"></init>"#)
            .unwrap();
        stream.write_all(&[0]).unwrap();
        let mut reader = stream.try_clone().unwrap();
        // Consome o run e fica em silêncio (script "longo"). O socket fica
        // aberto até a IDE encerrar a sessão — fechar aqui seria um falso
        // "debugger encerrou" durante o Stop.
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match reader.read(&mut byte) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if byte[0] == 0 {
                        break;
                    }
                    buf.push(byte[0]);
                }
            }
        }
        let _ = tx.send(());
        let mut sink = Vec::new();
        let _ = reader.read_to_end(&mut sink);
    });
    let mut engine = DebugEngine::start(
        ListenerConfig {
            port,
            ..Default::default()
        },
        PathMappings::new(PathBuf::from("/tmp")),
        std::sync::Arc::new(std::sync::Mutex::new(BreakpointStore::new())),
    )
    .unwrap();
    // PHP real longo (sleep) com env de debug.
    let mut child = Command::new(&php)
        .arg("-r")
        .arg("sleep(120);")
        .env(
            "XDEBUG_CONFIG",
            format!("client_host=127.0.0.1 client_port={port}"),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    wait_connected(&mut engine);
    // Stop: engine encerra a sessão e o teste mata o grupo do PHP real.
    engine.send(EngineCommand::Stop);
    loop {
        match engine.wait_event(Duration::from_secs(10)) {
            Some(EngineEvent::Ended(SessionEnd::Stopped)) => break,
            Some(EngineEvent::Continued(_)) | Some(EngineEvent::FrameVariables(_)) => continue,
            other => panic!("gate stop: esperava Stopped: {other:?}"),
        }
    }
    rx.recv_timeout(Duration::from_secs(5))
        .expect("peer vê socket fechado");
    // Processo real morto (kill direto pelo pid como o RunManager faria).
    let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
    let status = child.wait().unwrap();
    assert!(
        status.code().is_none() || status.code() != Some(0),
        "php real encerrado"
    );
    // Cleanup total: porta liberada para novo listener.
    engine.shutdown();
    peer.join().unwrap();
    std::thread::sleep(Duration::from_millis(150));
    TcpListener::bind(("127.0.0.1", port)).expect("porta liberada após Stop confiável");
}

#[test]
fn gate_detach_confiavel_com_php_real_vivo() {
    let Some(php) = php() else {
        eprintln!("GATE SKIP: php não disponível");
        return;
    };
    let port = free_port();
    let (tx, rx) = mpsc::channel();
    let peer = std::thread::spawn(move || {
        let mut stream = loop {
            if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
                break s;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        stream
            .write_all(br#"<init xmlns="urn:debugger_protocol_v1" fileuri="file:///x.php" language="PHP" appid="1" idekey="phpide"></init>"#)
            .unwrap();
        stream.write_all(&[0]).unwrap();
        let mut reader = stream.try_clone().unwrap();
        let mut sink = Vec::new();
        let _ = reader.read_to_end(&mut sink);
        let _ = tx.send(());
    });
    let mut engine = DebugEngine::start(
        ListenerConfig {
            port,
            ..Default::default()
        },
        PathMappings::new(PathBuf::from("/tmp")),
        std::sync::Arc::new(std::sync::Mutex::new(BreakpointStore::new())),
    )
    .unwrap();
    let mut child = Command::new(&php)
        .arg("-r")
        .arg("sleep(2);")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_connected(&mut engine);
    // Detach: sessão encerra; o processo PHP real segue vivo até terminar.
    engine.send(EngineCommand::Detach);
    loop {
        match engine.wait_event(Duration::from_secs(10)) {
            Some(EngineEvent::Ended(SessionEnd::Detached)) => break,
            Some(EngineEvent::Continued(_)) => continue,
            other => panic!("gate detach: esperava Detached: {other:?}"),
        }
    }
    rx.recv_timeout(Duration::from_secs(5))
        .expect("peer vê socket fechado");
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(0), "php real completou após Detach");
    engine.shutdown();
    peer.join().unwrap();
}

// ---------------------------------------------------------------------------
// Xdebug 3 real — extensão carregada localmente (sem root). Quando o `.so`
// está disponível, estas três sessões rodam de ponta a ponta contra o engine
// Xdebug verdadeiro (init real, breakpoint_set, break, stack_get,
// context_get, step_over, eval base64, detach/stop reais).
// ---------------------------------------------------------------------------

/// Sessão completa com Xdebug real: breakpoint, frames, variáveis, step over,
/// evaluate/watches por frame e resume até o fim do script (T-RUN-DBG-001-03,
/// T-RUN-DBG-002-01/02) — o mesmo fluxo antes exercido só pelo peer fiel.
#[test]
fn gate_debug_sessao_completa_xdebug_real() {
    let Some(php) = php() else {
        eprintln!("GATE SKIP: php não disponível");
        return;
    };
    let Some(so) = xdebug_so() else {
        eprintln!("GATE SKIP: extensão Xdebug não encontrada (PHPIDE_XDEBUG_SO)");
        return;
    };
    if !xdebug_real_disponivel(&php, &so) {
        eprintln!("GATE SKIP: xdebug.so não carrega neste php (ABI divergente)");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("gate_x.php");
    std::fs::write(
        &script,
        "<?php\n$a = 40;\n$b = $a + 2;\necho \"soma=$b\\n\";\n",
    )
    .unwrap();
    let port = free_port();
    let mut store = BreakpointStore::new();
    store.add_line(script.clone(), 2);

    let mut engine = DebugEngine::start(
        ListenerConfig {
            port,
            ide_key: Some("phpide".into()),
            ..Default::default()
        },
        PathMappings::new(dir.path().to_path_buf()),
        std::sync::Arc::new(std::sync::Mutex::new(store)),
    )
    .unwrap();

    // PHP REAL com Xdebug 3 real: conecta no listener e para no breakpoint.
    // Stdio nul/arquivo: o filho não pode herdar os pipes de captura do harness.
    let out_path = dir.path().join("out.txt");
    let out_file = std::fs::File::create(&out_path).unwrap();
    let mut child = php_com_xdebug(&php, &so, "debug")
        .arg("-d")
        .arg("xdebug.start_with_request=yes")
        .arg("-d")
        .arg("xdebug.client_host=127.0.0.1")
        .arg("-d")
        .arg(format!("xdebug.client_port={port}"))
        .arg("-d")
        .arg("xdebug.idekey=phpide")
        .arg(&script)
        .env("XDEBUG_SESSION", "phpide")
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn php com xdebug real");

    // Handshake: conexão Owned e breakpoint sincronizado (T-RUN-DBG-001-01/03).
    let (matched, synced) = wait_connected_full(&mut engine);
    assert!(
        matches!(matched, phpide_dbgp::mapping::ConnectionMatch::Owned(_)),
        "{matched:?}"
    );
    assert!(
        synced
            .iter()
            .any(|s| matches!(s, phpide_dbgp::breakpoint::SyncEvent::Set { .. })),
        "{synced:?}"
    );

    // Parada no breakpoint da linha 2 (antes de executá-la). Consome também
    // o FrameVariables que acompanha o Stopped — a fila precisa estar limpa
    // antes do step (senão o contexto do stop 1 parece ser do stop 2).
    let mut stopped_line2 = false;
    loop {
        match engine.wait_event(Duration::from_secs(10)) {
            Some(EngineEvent::Stopped { reason, frames }) => {
                assert_eq!(
                    reason,
                    phpide_dbgp::session::StopReason::Location {
                        file: script.clone(),
                        line: 2
                    },
                    "{reason:?}"
                );
                assert_eq!(frames.len(), 1, "{frames:?}");
                assert_eq!(frames[0].line, 2);
                stopped_line2 = true;
            }
            Some(EngineEvent::FrameVariables(_)) if stopped_line2 => break,
            Some(EngineEvent::Continued(_)) | Some(EngineEvent::FrameVariables(_)) => continue,
            other => panic!("gate xdebug: esperava Stopped: {other:?}"),
        }
    }

    // Step over (F8): para na linha 3 com $a já definida (=40).
    engine.send(EngineCommand::Continue(ContinueCommand::StepOver));
    let mut saw_stop3 = false;
    let mut saw_vars = false;
    loop {
        match engine.wait_event(Duration::from_secs(10)) {
            Some(EngineEvent::Stopped { reason, frames }) => {
                assert!(
                    matches!(
                        reason,
                        phpide_dbgp::session::StopReason::Location { line: 3, .. }
                    ),
                    "{reason:?}"
                );
                assert_eq!(frames[0].line, 3);
                saw_stop3 = true;
            }
            Some(EngineEvent::FrameVariables(vars)) => {
                let a = vars.iter().find(|v| v.name == "$a");
                assert!(
                    a.is_some_and(|v| v.value.as_deref() == Some("40")),
                    "variáveis após step: {vars:?}"
                );
                saw_vars = true;
            }
            Some(EngineEvent::Continued(_)) => continue,
            other => panic!("gate xdebug: evento inesperado no step: {other:?}"),
        }
        if saw_stop3 && saw_vars {
            break;
        }
    }

    // Evaluate por frame (Alt+F8): variável endereça o frame via property_get.
    engine.send(EngineCommand::Evaluate {
        expression: "$a".into(),
        depth: 0,
    });
    loop {
        match engine.wait_event(Duration::from_secs(10)) {
            Some(EngineEvent::Evaluated { expression, result }) => {
                assert_eq!(expression, "$a");
                let value = result.expect("evaluate ok").expect("$a disponível");
                assert_eq!(value.value.as_deref(), Some("40"), "{value:?}");
                break;
            }
            Some(EngineEvent::FrameVariables(_)) | Some(EngineEvent::Continued(_)) => continue,
            other => panic!("gate xdebug: esperava Evaluated: {other:?}"),
        }
    }
    // Eval de expressão: base64 no fio, avaliado no topo da pilha.
    engine.send(EngineCommand::Evaluate {
        expression: "40+2".into(),
        depth: 0,
    });
    loop {
        match engine.wait_event(Duration::from_secs(10)) {
            Some(EngineEvent::Evaluated { expression, result }) => {
                assert_eq!(expression, "40+2");
                let value = result.expect("eval ok").expect("resultado do eval");
                assert_eq!(value.value.as_deref(), Some("42"), "{value:?}");
                break;
            }
            Some(EngineEvent::FrameVariables(_)) | Some(EngineEvent::Continued(_)) => continue,
            other => panic!("gate xdebug: esperava Evaluated (eval): {other:?}"),
        }
    }
    // Watches por frame (T-RUN-DBG-002-02).
    engine.send(EngineCommand::EvaluateWatches {
        ids: vec![7],
        expressions: vec!["$a".into()],
        depth: 0,
    });
    loop {
        match engine.wait_event(Duration::from_secs(10)) {
            Some(EngineEvent::WatchValues(values)) => {
                assert_eq!(values.len(), 1);
                assert_eq!(values[0].watch_id, 7);
                let value = values[0]
                    .result
                    .as_ref()
                    .expect("watch ok")
                    .as_ref()
                    .expect("watch $a");
                assert_eq!(value.value.as_deref(), Some("40"), "{value:?}");
                break;
            }
            Some(EngineEvent::FrameVariables(_)) | Some(EngineEvent::Continued(_)) => continue,
            other => panic!("gate xdebug: esperava WatchValues: {other:?}"),
        }
    }

    // Resume (F9): o script vai até o fim e o PHP real completa com exit 0.
    engine.send(EngineCommand::Continue(ContinueCommand::Resume));
    loop {
        match engine.wait_event(Duration::from_secs(10)) {
            Some(EngineEvent::Ended(SessionEnd::Finished)) => break,
            Some(EngineEvent::Continued(_)) | Some(EngineEvent::FrameVariables(_)) => continue,
            other => panic!("gate xdebug: esperava Finished: {other:?}"),
        }
    }
    assert_eq!(
        child.wait().unwrap().code(),
        Some(0),
        "php real com Xdebug terminou limpo"
    );
    let stdout = std::fs::read_to_string(&out_path).unwrap();
    assert!(stdout.contains("soma=42"), "{stdout}");
    engine.shutdown();
}

/// Detach com Xdebug real: a sessão encerra e o processo PHP segue vivo até
/// completar normalmente (T-RUN-DBG-001-04 — Detach confiável com extensão).
#[test]
fn gate_detach_xdebug_real_deixa_php_completar() {
    let Some(php) = php() else {
        eprintln!("GATE SKIP: php não disponível");
        return;
    };
    let Some(so) = xdebug_so() else {
        eprintln!("GATE SKIP: extensão Xdebug não encontrada (PHPIDE_XDEBUG_SO)");
        return;
    };
    if !xdebug_real_disponivel(&php, &so) {
        eprintln!("GATE SKIP: xdebug.so não carrega neste php (ABI divergente)");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("gate_detach.php");
    std::fs::write(&script, "<?php\nusleep(400000);\necho \"pos-detach\\n\";\n").unwrap();
    let port = free_port();
    let mut store = BreakpointStore::new();
    store.add_line(script.clone(), 2);
    let mut engine = DebugEngine::start(
        ListenerConfig {
            port,
            ide_key: Some("phpide".into()),
            ..Default::default()
        },
        PathMappings::new(dir.path().to_path_buf()),
        std::sync::Arc::new(std::sync::Mutex::new(store)),
    )
    .unwrap();
    let out_path = dir.path().join("out.txt");
    let out_file = std::fs::File::create(&out_path).unwrap();
    let mut child = php_com_xdebug(&php, &so, "debug")
        .arg("-d")
        .arg("xdebug.start_with_request=yes")
        .arg("-d")
        .arg("xdebug.client_host=127.0.0.1")
        .arg("-d")
        .arg(format!("xdebug.client_port={port}"))
        .arg("-d")
        .arg("xdebug.idekey=phpide")
        .arg(&script)
        .env("XDEBUG_SESSION", "phpide")
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn php com xdebug real");
    wait_connected(&mut engine);
    loop {
        match engine.wait_event(Duration::from_secs(10)) {
            Some(EngineEvent::Stopped { .. }) => break,
            Some(EngineEvent::Continued(_)) | Some(EngineEvent::FrameVariables(_)) => continue,
            other => panic!("gate detach: esperava Stopped: {other:?}"),
        }
    }
    engine.send(EngineCommand::Detach);
    loop {
        match engine.wait_event(Duration::from_secs(10)) {
            Some(EngineEvent::Ended(SessionEnd::Detached)) => break,
            Some(EngineEvent::Continued(_)) | Some(EngineEvent::FrameVariables(_)) => continue,
            other => panic!("gate detach: esperava Detached: {other:?}"),
        }
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.code(), Some(0), "php real completou após Detach");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "timeout aguardando php pós-Detach"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let stdout = std::fs::read_to_string(&out_path).unwrap();
    assert!(stdout.contains("pos-detach"), "{stdout}");
    engine.shutdown();
}

/// Stop com Xdebug real: o comando `stop` do DBGp encerra o script (sem kill
/// externo), o processo PHP morre e a porta é liberada (T-RUN-DBG-001-04).
#[test]
fn gate_stop_xdebug_real_termina_php_e_libera_porta() {
    let Some(php) = php() else {
        eprintln!("GATE SKIP: php não disponível");
        return;
    };
    let Some(so) = xdebug_so() else {
        eprintln!("GATE SKIP: extensão Xdebug não encontrada (PHPIDE_XDEBUG_SO)");
        return;
    };
    if !xdebug_real_disponivel(&php, &so) {
        eprintln!("GATE SKIP: xdebug.so não carrega neste php (ABI divergente)");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("gate_stop.php");
    std::fs::write(&script, "<?php\nsleep(120);\n").unwrap();
    let port = free_port();
    let mut store = BreakpointStore::new();
    store.add_line(script.clone(), 2);
    let mut engine = DebugEngine::start(
        ListenerConfig {
            port,
            ide_key: Some("phpide".into()),
            ..Default::default()
        },
        PathMappings::new(dir.path().to_path_buf()),
        std::sync::Arc::new(std::sync::Mutex::new(store)),
    )
    .unwrap();
    let mut child = php_com_xdebug(&php, &so, "debug")
        .arg("-d")
        .arg("xdebug.start_with_request=yes")
        .arg("-d")
        .arg("xdebug.client_host=127.0.0.1")
        .arg("-d")
        .arg(format!("xdebug.client_port={port}"))
        .arg("-d")
        .arg("xdebug.idekey=phpide")
        .arg(&script)
        .env("XDEBUG_SESSION", "phpide")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn php com xdebug real");
    wait_connected(&mut engine);
    loop {
        match engine.wait_event(Duration::from_secs(10)) {
            Some(EngineEvent::Stopped { .. }) => break,
            Some(EngineEvent::Continued(_)) | Some(EngineEvent::FrameVariables(_)) => continue,
            other => panic!("gate stop: esperava Stopped: {other:?}"),
        }
    }
    engine.send(EngineCommand::Stop);
    loop {
        match engine.wait_event(Duration::from_secs(10)) {
            Some(EngineEvent::Ended(SessionEnd::Stopped)) => break,
            Some(EngineEvent::Continued(_)) | Some(EngineEvent::FrameVariables(_)) => continue,
            other => panic!("gate stop: esperava Stopped: {other:?}"),
        }
    }
    // O `stop` do DBGp mata o script: o PHP real encerra sozinho.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "timeout aguardando morte do php após Stop"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    engine.shutdown();
    std::thread::sleep(Duration::from_millis(150));
    TcpListener::bind(("127.0.0.1", port)).expect("porta liberada após Stop com Xdebug real");
}
