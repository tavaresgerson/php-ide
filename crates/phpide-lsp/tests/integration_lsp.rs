//! Teste de integração: sobe o servidor LSP real (intelephense local ou o que
//! `PHPIDE_LSP` apontar) e espera diagnósticos de sintaxe para um PHP quebrado.
//! Se nenhum servidor estiver instalado, o teste é pulado com aviso.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use phpide_lsp::detect_server;
use phpide_lsp::LspClient;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn server_reports_php_syntax_diagnostics() {
    let ws_root = workspace_root();
    let Some((cmd, args)): Option<(String, Vec<String>)> = detect_server(&ws_root) else {
        eprintln!("[skip] nenhum servidor LSP encontrado (instale com: npm --prefix third_party install intelephense)");
        return;
    };
    eprintln!("[lsp] usando servidor: {cmd} {}", args.join(" "));

    let root = std::env::temp_dir().join(format!("phpide-lsp-it-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("index.php");
    let text = "<?php\nfunction broken( {\n    return 1;\n";
    std::fs::write(&file, text).unwrap();

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (client, init_rx) = LspClient::spawn(&cmd, &arg_refs, &root).expect("spawn");

    // 1. initialize deve responder com capabilities.
    let init = init_rx
        .recv_timeout(Duration::from_secs(25))
        .expect("timeout no initialize");
    assert!(
        init.get("capabilities").is_some(),
        "resposta sem capabilities: {init}"
    );
    eprintln!("[lsp] capabilities ok");

    client.notify("initialized", serde_json::json!({}));
    client.did_open(&file, text, 1);

    // 2. Publicar diagnóstico de sintaxe (pode levar alguns segundos na 1ª indexação).
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut found = Vec::new();
    while Instant::now() < deadline {
        if let Some(items) = client.shared.diagnostics.lock().unwrap().get(&file).cloned() {
            if !items.is_empty() {
                found = items;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }

    client.stop();
    let _ = std::fs::remove_dir_all(&root);

    assert!(!found.is_empty(), "servidor não publicou diagnósticos");
    assert!(
        found.iter().any(|d| d.severity == 1),
        "esperava ao menos um erro de sintaxe, veio: {found:?}"
    );
    eprintln!("[lsp] diagnósticos recebidos: {}", found.len());
}
