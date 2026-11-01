//! Teste de integração do fluxo DBGp com um "xdebug falso": um socket TCP que
//! fala o suficiente do protocolo (init -> breakpoint_set -> run -> stack_get
//! -> context_get -> detach), sem precisar do Xdebug instalado.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use phpide_dbgp::{run_with_breakpoints, DebugMsg};

fn fake_xdebug(port: u16) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        // A IDE é quem escuta; o "xdebug" conecta de volta (com retry).
        let mut stream = loop {
            match std::net::TcpStream::connect(("127.0.0.1", port)) {
                Ok(s) => break s,
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        };
        let mut reader = stream.try_clone().unwrap();
        let init = r#"<?xml version="1.0"?><init xmlns="urn:debugger_protocol_v1" fileuri="file:///tmp/t.php" language="PHP" protocol_version="1.0" appid="999" idekey="phpide"></init>"#;
        stream.write_all(init.as_bytes()).unwrap();
        stream.write_all(&[0]).unwrap();

        let mut tx = 1u32;
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        for expected in ["breakpoint_set", "run", "stack_get", "context_get", "detach"] {
            buf.clear();
            while reader.read(&mut byte).unwrap() > 0 {
                if byte[0] == 0 {
                    break;
                }
                buf.push(byte[0]);
            }
            let cmd = String::from_utf8_lossy(&buf).into_owned();
            assert!(cmd.starts_with(expected), "esperava {expected}, veio: {cmd}");
            tx += 1;
            let response = match expected {
                "breakpoint_set" => format!(r#"<response command="breakpoint_set" transaction_id="{tx}" id="10" success="1"></response>"#),
                "run" => format!(r#"<response command="run" transaction_id="{tx}" status="break" reason="ok"><xdebug:message filename="file:///tmp/t.php" lineno="5"/></response>"#),
                "stack_get" => format!(r#"<response command="stack_get" transaction_id="{tx}"><stack where="{{main}}" level="0" type="file" filename="file:///tmp/t.php" lineno="4"/></response>"#),
                "context_get" => format!(r#"<response command="context_get" transaction_id="{tx}"><property name="$total" fullname="$total" type="int">42</property></response>"#),
                "detach" => format!(r#"<response command="detach" transaction_id="{tx}" status="stopping" reason="ok"></response>"#),
                _ => unreachable!(),
            };
            stream.write_all(response.as_bytes()).unwrap();
            stream.write_all(&[0]).unwrap();
        }
    })
}

#[test]
fn full_debug_session_with_fake_xdebug() {
    // acha uma porta livre
    let probe = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let server = fake_xdebug(port);
    let (tx, rx) = mpsc::channel();
    let bps = vec![(std::path::PathBuf::from("/tmp/t.php"), 4u32)];
    std::thread::spawn(move || {
        let _ = run_with_breakpoints(port, Duration::from_secs(15), &bps, tx);
    });

    let mut lines = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(DebugMsg::Line(l)) => {
                eprintln!("[test] {l}");
                lines.push(l);
            }
            Ok(DebugMsg::Done) => break,
            Err(_) => assert!(Instant::now() < deadline, "timeout no fluxo dbg"),
        }
    }
    server.join().unwrap();

    let joined = lines.join("\n");
    assert!(joined.contains("xdebug conectado"), "{joined}");
    assert!(joined.contains("breakpoint") && joined.contains("ok"), "{joined}");
    assert!(joined.contains("status=break"), "{joined}");
    assert!(joined.contains("parado em file:///tmp/t.php"), "{joined}");
    assert!(joined.contains("$total"), "{joined}");
}
