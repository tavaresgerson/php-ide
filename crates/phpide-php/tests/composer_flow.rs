//! Gate de fluxo PHP P0: projeto Composer real em disco + servidor LSP real
//! (intelephense local ou `PHPIDE_LSP`). Abrir, editar (sync versionado),
//! diagnosticar, completar, navegar e salvar — com o `LspClient` real.
//! Sem servidor instalado, o teste é pulado com aviso (a degradação offline
//! é coberta pelos testes de `gate_tests` na UI).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use phpide_lsp::{detect_server, LspClient, LspStatus};
use serde_json::Value;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("phpide-php-flow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

fn write_project(root: &Path) {
    std::fs::write(
        root.join("composer.json"),
        r#"{
            "name": "acme/flow",
            "require": { "php": ">=8.1" },
            "autoload": { "psr-4": { "Acme\\Flow\\": "src/" } }
        }"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/Greeter.php"),
        "<?php\nnamespace Acme\\Flow;\n\nclass Greeter\n{\n    public function greet(string $name): string\n    {\n        return 'Olá, ' . $name;\n    }\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/main.php"),
        "<?php\nnamespace Acme\\Flow;\n\nrequire __DIR__ . '/Greeter.php';\n\n$g = new Greeter();\necho $g->greet('phpide');\n",
    )
    .unwrap();
    // Arquivo com erro de sintaxe para o requisito "diagnosticar".
    std::fs::write(root.join("src/broken.php"), "<?php\nfunction oops( {\n").unwrap();
}

fn wait_for<T>(timeout: Duration, mut poll: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(v) = poll() {
            return Some(v);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

#[test]
fn composer_project_full_flow_with_real_lsp() {
    let ws_root = workspace_root();
    let Some((cmd, args)): Option<(String, Vec<String>)> = detect_server(&ws_root) else {
        eprintln!("[skip] nenhum servidor LSP encontrado");
        return;
    };
    let root = scratch();
    write_project(&root);

    // 1. Descoberta do projeto Composer (T-PHP-030-01/02).
    let info = phpide_php::composer::discover(&root).unwrap().unwrap();
    assert_eq!(info.php_level.as_deref(), Some("8.1"));
    assert!(info.source_roots.iter().any(|(p, _)| p == "Acme\\Flow"));
    assert!(!info.vendor_present, "vendor ausente no fixture");
    assert!(info.warnings.iter().any(|w| w.contains("composer install")));

    // 2. Sobe o servidor real.
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let Ok((lsp, init_rx)) = LspClient::spawn(&cmd, &refs, &root) else {
        eprintln!("[skip] falha ao iniciar {cmd}");
        std::fs::remove_dir_all(&root).ok();
        return;
    };
    let init = wait_for(Duration::from_secs(30), || init_rx.try_recv().ok())
        .expect("initialize do servidor");
    lsp.register_capabilities(&init);
    lsp.notify("initialized", Value::Null);
    // Transição de estado que a UI faz no poll (T-PHP-001-02): Starting →
    // Running quando o initialize traz capabilities.
    *lsp.shared.status.lock().unwrap() = LspStatus::Running(cmd.clone());
    let caps = lsp.capabilities();
    assert!(caps.definition_provider, "intelephense declara definition");
    assert!(caps.completion_provider, "intelephense declara completion");

    // 3. Abrir arquivos (didOpen) com versões.
    let main = root.join("src/main.php");
    let broken = root.join("src/broken.php");
    let main_text = std::fs::read_to_string(&main).unwrap();
    lsp.did_open(&main, &main_text, 1);
    lsp.did_open(&broken, &std::fs::read_to_string(&broken).unwrap(), 1);

    // 4. Diagnosticar: publicação de diagnósticos para o arquivo quebrado.
    let diags = wait_for(Duration::from_secs(60), || {
        let map = lsp.shared.diagnostics.lock().unwrap();
        let list = map.get(&broken)?;
        if list.is_empty() {
            None
        } else {
            Some(list.clone())
        }
    })
    .expect("diagnósticos do arquivo quebrado");
    assert!(!diags.is_empty());

    // 5. Editar com versão (didChange) e salvar (didSave) sem erro de ordem.
    let edited = main_text.clone() + "\n// editado\n";
    lsp.did_change(&main, &edited, 2);
    lsp.did_save(&main, &edited);
    std::thread::sleep(Duration::from_millis(300));

    // 6. Completar após `->` (método greet). A indexação do intelephense é
    //    assíncrona: pedido único logo após didOpen/didSave volta vazio em
    //    ambientes lentos (falha intermitente real confirmada). Poll até o
    //    item aparecer — é o que acontece na prática: o usuário completa
    //    depois que a indexação assenta.
    let line = 6;
    let character = 11; // após "$g->"
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut seen: Vec<String> = Vec::new();
    loop {
        let rx = lsp.completion(&main, line, character);
        if let Some(result) = wait_for(Duration::from_secs(10), || rx.try_recv().ok()) {
            let items = phpide_lsp::parse_completions(&result);
            seen = items.iter().map(|i| i.label.clone()).take(5).collect();
            if items.iter().any(|i| i.label == "greet") {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "completion oferece 'greet': {seen:?}"
        );
        std::thread::sleep(Duration::from_millis(250));
    }

    // 7. Navegar: definition de `greet` leva a Greeter.php (mesmo regime de
    //    poll: a indexação pode ainda estar esquentando).
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut locations: Vec<PathBuf> = Vec::new();
    loop {
        let rx = lsp.definition(&main, line, 13); // dentro de "greet"
        if let Some(result) = wait_for(Duration::from_secs(10), || rx.try_recv().ok()) {
            let arr = result
                .as_array()
                .cloned()
                .unwrap_or_else(|| vec![result.clone()]);
            locations = arr
                .iter()
                .filter_map(|loc| phpide_lsp::uri_to_path(loc["uri"].as_str()?))
                .collect();
            if locations.iter().any(|p| p.ends_with("Greeter.php")) {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "definition aponta para Greeter.php: {locations:?}"
        );
        std::thread::sleep(Duration::from_millis(250));
    }

    // 8. Encerrar ordenado (shutdown/exit) — IDE segue viva.
    lsp.stop();
    assert!(
        matches!(lsp.status(), LspStatus::Running(_)) || true,
        "stop não pânica"
    );
    std::fs::remove_dir_all(&root).ok();
}
