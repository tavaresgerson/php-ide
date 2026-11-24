//! Gate Fase 7 — Frontend essencial: projeto Vite + TypeScript real,
//! editado/diagnosticado/navegado pelos Language Servers reais
//! (typescript-language-server, vscode-html/css). Sem os servidores no
//! ambiente o gate se autopula (NFR-003 — ausência ≠ falha).

use phpide_lsp::LspStatus;

fn have(bin: &str) -> bool {
    crate::fe_servers::FeServers::empty();
    // descobre via find_bin do módulo fe_servers (teste interno ao bin).
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    [
        root.join("third_party/node_modules/.bin").join(bin),
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../third_party/node_modules/.bin")
            .join(bin),
    ]
    .iter()
    .any(|p| p.exists())
        || std::process::Command::new("which")
            .arg(bin)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
}

fn wait_running(srv: &mut crate::fe_servers::FeServer, timeout: std::time::Duration) -> bool {
    let t0 = std::time::Instant::now();
    loop {
        crate::fe_servers::poll_init(srv);
        if srv.initialized && matches!(srv.client.status(), LspStatus::Running(_)) {
            return true;
        }
        if t0.elapsed() > timeout {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
}

#[test]
fn gate_vite_typescript_edit_diagnose_navigate() {
    if !have("typescript-language-server") || !have("vscode-html-language-server") {
        eprintln!("GATE SKIP: servidores frontend ausentes");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    // Fixture Vite mínimo + node_modules presente (biblioteca, não árvore).
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("node_modules")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"vite-demo","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true,"target":"ES2020","module":"ESNext"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("index.html"),
        "<!doctype html>\n<html><body>\n  <div>oi\n</body></html>\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/math.ts"),
        "export function soma(a: number, b: number): number {\n  return a + b;\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/main.ts"),
        "import { soma } from './math';\nconst total: number = soma(1, 'x');\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/style.css"),
        "body { color: #ff8800; background: rgb(0, 0, 0); }\n",
    )
    .unwrap();

    // FE-001-04: node_modules presente NÃO entra na árvore do projeto.
    let entries = phpide_core::scan(&root);
    assert!(
        !entries.iter().any(|e| e.path.ends_with("node_modules")),
        "node_modules fora da árvore"
    );

    let files: Vec<(std::path::PathBuf, String)> =
        ["src/main.ts", "src/math.ts", "src/style.css", "index.html"]
            .iter()
            .map(|r| (root.join(r), std::fs::read_to_string(root.join(r)).unwrap()))
            .collect();
    let mut fe = crate::fe_servers::detect_and_spawn(&root);
    {
        let ts = fe.ts.as_mut().expect("typescript-language-server");
        assert!(
            wait_running(ts, std::time::Duration::from_secs(30)),
            "TS-LS running"
        );
    }

    // Sync (FE-001-01: didOpen) dos arquivos fonte por servidor.
    let rev = 1u64;
    if let Some(ts) = fe.ts.as_mut() {
        ts.sync_buffers(
            files
                .iter()
                .filter(|(p, _)| p.extension().and_then(|e| e.to_str()) == Some("ts"))
                .map(|(p, t)| (p, t.as_str(), rev)),
        );
    }
    if let Some(css) = fe.css.as_mut() {
        if wait_running(css, std::time::Duration::from_secs(20)) {
            css.sync_buffers(
                files
                    .iter()
                    .filter(|(p, _)| p.extension().and_then(|e| e.to_str()) == Some("css"))
                    .map(|(p, t)| (p, t.as_str(), rev)),
            );
            // Color preview (FE-001-03): documentColor real do servidor.
            let rx = css.client.request(
                "textDocument/documentColor",
                serde_json::json!({
                    "textDocument": { "uri": phpide_lsp::path_to_uri(&root.join("src/style.css")) }
                }),
            );
            let colors = wait_value(rx, std::time::Duration::from_secs(10));
            let arr = colors.as_array().cloned().unwrap_or_default();
            assert!(arr.len() >= 2, "cores reais do CSS: {arr:?}");
        }
    }
    if let Some(html) = fe.html.as_mut() {
        if wait_running(html, std::time::Duration::from_secs(20)) {
            html.sync_buffers(
                files
                    .iter()
                    .filter(|(p, _)| p.extension().and_then(|e| e.to_str()) == Some("html"))
                    .map(|(p, t)| (p, t.as_str(), rev)),
            );
        }
    }

    // FE-001-01/FE-002-02: diagnóstico REAL (erro de tipo em main.ts).
    let t0 = std::time::Instant::now();
    let mut diags = Vec::new();
    while t0.elapsed() < std::time::Duration::from_secs(15) {
        diags = fe.drain_diagnostics();
        if diags
            .iter()
            .any(|(p, d)| p.ends_with("main.ts") && !d.is_empty())
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let (path, items) = diags
        .iter()
        .find(|(p, _)| p.ends_with("main.ts"))
        .cloned()
        .expect("diagnósticos publicados para main.ts");
    assert!(
        items
            .iter()
            .any(|d| d.severity == 1 || !d.message.is_empty()),
        "diagnóstico real em {path:?}: {items:?}"
    );

    // FE-001-02: completion real em main.ts.
    let rx = {
        let ts = fe.ts.as_ref().unwrap();
        ts.client.request(
            "textDocument/completion",
            serde_json::json!({
                "textDocument": { "uri": phpide_lsp::path_to_uri(&root.join("src/main.ts")) },
                "position": { "line": 0, "character": 10 }
            }),
        )
    };
    let comp = wait_value(rx, std::time::Duration::from_secs(10));
    let items = phpide_lsp::parse_completions(&comp);
    assert!(!items.is_empty(), "completion real: {comp:?}");

    // FE-001-02: navegação real — definition de 'soma' leva a math.ts.
    let rx = fe
        .ts
        .as_ref()
        .unwrap()
        .client
        .definition(&root.join("src/main.ts"), 0, 9);
    let loc = wait_value(rx, std::time::Duration::from_secs(10));
    let locs = crate::intelligence::parse_locations(&loc);
    assert!(
        locs.iter().any(|l| l.path.ends_with("math.ts")),
        "definition navega para math.ts: {loc:?}"
    );

    // FE-002-02: Go to Type Declaration (typeDefinition).
    let rx = fe
        .ts
        .as_ref()
        .unwrap()
        .client
        .type_definition(&root.join("src/main.ts"), 1, 7);
    let _td = wait_value(rx, std::time::Duration::from_secs(10));
}

fn pump_app(app: &mut crate::PhpIdeApp) {
    app.poll_lsp();
    app.poll_fe();
    app.poll_routed_requests();
    app.poll_hints();
    app.poll_completion();
}

fn wait_app(
    label: &str,
    timeout: std::time::Duration,
    app: &mut crate::PhpIdeApp,
    mut ready: impl FnMut(&crate::PhpIdeApp) -> bool,
) {
    let started = std::time::Instant::now();
    while started.elapsed() < timeout {
        pump_app(app);
        if ready(app) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    panic!("timeout aguardando {label}");
}

fn typescript_fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"routing-gate","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true,"target":"ES2020","module":"ESNext"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("math.ts"),
        "export function soma(a: number, b: number): number { return a + b; }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("main.ts"),
        "import { soma } from './math';\nconst total: number = soma(1, 2);\n",
    )
    .unwrap();
    (dir, root.join("main.ts"), root.join("math.ts"))
}

#[test]
fn gate_application_typescript_without_php_completion_navigation_and_hints() {
    if !have("typescript-language-server") {
        panic!("typescript-language-server obrigatório para este gate");
    }
    let (_dir, main, math) = typescript_fixture();
    let root = main.parent().unwrap().to_path_buf();
    let mut app = crate::PhpIdeApp::new(root, false, vec![main.clone()]);
    app.lsp = None;
    app.lsp_init = None;
    wait_app(
        "inicialização",
        std::time::Duration::from_secs(30),
        &mut app,
        |app| {
            app.fe
                .ts
                .as_ref()
                .map(|server| server.initialized)
                .unwrap_or(false)
        },
    );
    assert!(app.lsp.is_none());

    app.cursor_char = 10;
    app.trigger_completion();
    assert_eq!(app.completion.origin, crate::routing::LspOrigin::TypeScript);
    wait_app(
        "completion",
        std::time::Duration::from_secs(15),
        &mut app,
        |app| !app.completion.loading(),
    );
    app.completion.set_query(String::new());
    assert!(!app.completion.visible().is_empty());
    app.close_completion();

    app.cursor_char = std::fs::read_to_string(&main)
        .unwrap()
        .find("./math")
        .unwrap()
        + 3;
    app.request_definition();
    assert!(
        app.pending_requests
            .has_operation(crate::routing::RequestOperation::Definition),
        "{}",
        app.ws.message
    );
    wait_app(
        "definition",
        std::time::Duration::from_secs(15),
        &mut app,
        |app| {
            app.ws
                .active_buffer()
                .and_then(|buffer| buffer.path.as_deref())
                == Some(math.as_path())
        },
    );

    app.ws.open_file(&main);
    app.cursor_char = std::fs::read_to_string(&main)
        .unwrap()
        .find("total")
        .unwrap();
    app.request_type_definition();
    wait_app(
        "typeDefinition",
        std::time::Duration::from_secs(15),
        &mut app,
        |app| {
            !app.pending_requests
                .has_operation(crate::routing::RequestOperation::TypeDefinition)
        },
    );
    let revision = app.ws.active_buffer().unwrap().document.revision();
    wait_app(
        "hints",
        std::time::Duration::from_secs(15),
        &mut app,
        |app| {
            app.hints_path.as_deref() == Some(main.as_path())
                && app.hints.current == Some(crate::hints::Revision(revision))
        },
    );

    app.request_definition();
    app.ws
        .active_buffer_mut()
        .unwrap()
        .insert(0, "// edição local\n");
    std::thread::sleep(std::time::Duration::from_millis(200));
    pump_app(&mut app);
    assert_eq!(
        app.ws
            .active_buffer()
            .and_then(|buffer| buffer.path.as_deref()),
        Some(main.as_path()),
        "resposta de definição obsoleta não pode navegar"
    );
}

#[test]
fn gate_application_php_and_typescript_requests_stay_on_their_clients() {
    if !have("typescript-language-server") || !have("intelephense") {
        panic!("Intelephense e TypeScript são obrigatórios para este gate");
    }
    let (_dir, main, math) = typescript_fixture();
    let root = main.parent().unwrap().to_path_buf();
    let php = root.join("a.php");
    std::fs::write(&php, "<?php\nclass Greeter { public function greet(): void {} }\n$g = new Greeter();\n$g->greet();\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../third_party"),
        root.join("third_party"),
    )
    .unwrap();
    let mut app = crate::PhpIdeApp::new(root, false, vec![main.clone(), php.clone()]);
    wait_app(
        "inicialização",
        std::time::Duration::from_secs(30),
        &mut app,
        |app| {
            app.lsp
                .as_ref()
                .map(|client| matches!(client.status(), LspStatus::Running(_)))
                .unwrap_or(false)
                && app
                    .fe
                    .ts
                    .as_ref()
                    .map(|server| server.initialized)
                    .unwrap_or(false)
        },
    );

    app.ws.open_file_ex(&main, false);
    app.ws.active_buffer_mut().unwrap().preview = false;
    pump_app(&mut app);
    app.cursor_char = std::fs::read_to_string(&main)
        .unwrap()
        .find("./math")
        .unwrap()
        + 3;
    app.request_definition();
    app.ws.open_file_ex(&php, false);
    app.ws.active_buffer_mut().unwrap().preview = false;
    pump_app(&mut app);
    app.cursor_char = std::fs::read_to_string(&php)
        .unwrap()
        .rfind("greet")
        .unwrap();
    app.request_definition();
    assert!(
        app.pending_requests.len() >= 2,
        "PHP e TS devem permanecer pendentes juntos"
    );
    wait_app(
        "multi definitions",
        std::time::Duration::from_secs(20),
        &mut app,
        |app| {
            !app.pending_requests
                .has_operation(crate::routing::RequestOperation::Definition)
        },
    );
    assert!(
        app.ws
            .buffers
            .iter()
            .any(|buffer| buffer.path.as_deref() == Some(math.as_path())),
        "resposta TS não chegou a math.ts: {:?}",
        app.ws
            .buffers
            .iter()
            .map(|buffer| &buffer.path)
            .collect::<Vec<_>>()
    );
    assert!(
        app.ws
            .buffers
            .iter()
            .any(|buffer| buffer.path.as_deref() == Some(php.as_path())),
        "resposta PHP não foi consumida: {:?}",
        app.ws
            .buffers
            .iter()
            .map(|buffer| &buffer.path)
            .collect::<Vec<_>>()
    );
}

fn wait_value(
    rx: std::sync::mpsc::Receiver<serde_json::Value>,
    timeout: std::time::Duration,
) -> serde_json::Value {
    let t0 = std::time::Instant::now();
    loop {
        if let Ok(v) = rx.try_recv() {
            return v;
        }
        assert!(t0.elapsed() < timeout, "timeout aguardando resposta LSP");
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
}
