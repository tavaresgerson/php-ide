//! Integração real do Test Runner — PHPUnit 11 (phar) sobre fixture de
//! projeto (`T-RUN-002-03`, `T-RUN-TST-001-01/02`, `T-RUN-TST-002-01`).
//!
//! Ferramenta real: `phpunit.phar` do PHPUnit 11.5 (PHP 8.2). O teste procura
//! o runner em `PHPIDE_PHPUNIT` → `<workspace>/target/tmp/phpunit.phar`; se
//! não houver runner ou PHP, o teste imprime o motivo e retorna (ausência de
//! ferramenta é diferenciada de falha — NFR-003).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use phpide_run::config::{RunConfig, RunKind, TestScope};
use phpide_run::phpunit::{
    build_command, consolidate, resolve_phpunit, TeamCityParser, TestStatus,
};
use phpide_run::session::SessionState;
use phpide_run::{BuildContext, RunManager};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/phpunit-sample");

fn php() -> Option<String> {
    let php = std::env::var("PHPIDE_PHP").unwrap_or_else(|_| "php".into());
    let ok = Command::new(&php)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    ok.then_some(php)
}

fn phpunit() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("PHPIDE_PHPUNIT") {
        if !explicit.is_empty() {
            let p = PathBuf::from(explicit);
            if p.exists() {
                return Some(p);
            }
        }
    }
    let candidate = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/phpunit.phar");
    candidate.exists().then_some(candidate)
}

/// Extensão Xdebug carregável sem root: `.deb` do Debian (mesma ABI do PHP
/// 8.2 NTS) extraído em `target/tmp/xdebug-root` (ver gate_run_debug.rs).
/// Override por `PHPIDE_XDEBUG_SO`.
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

fn ctx(php: &str, phpunit: &Path) -> BuildContext {
    BuildContext {
        php: php.into(),
        composer: None,
        phpunit: Some(phpunit.to_string_lossy().into_owned()),
    }
}

fn cfg(scope: TestScope) -> RunConfig {
    RunConfig::new(
        format!("it-{:?}", scope),
        "IT PHPUnit",
        RunKind::PhpUnit {
            scope,
            config: None,
            bootstrap: None,
            runner_options: vec![],
            coverage: false,
        },
    )
}

fn run_and_tree(
    root: &Path,
    config: RunConfig,
    ctx: &BuildContext,
) -> (phpide_run::TestTree, SessionState, Option<i32>, String) {
    let cwd = root.to_path_buf();
    let (spec, _plan) = build_command(&config, root, ctx, cwd).expect("build do comando");
    let mut manager = RunManager::new();
    let id = manager.start(config, spec, None);
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut parser = TeamCityParser::new();
    let mut output = String::new();
    loop {
        manager.poll();
        let s = manager.session(id).unwrap();
        let known = output.len();
        let full = s.output();
        if full.len() > known {
            for line in full[known..].lines() {
                parser.feed_line(line);
            }
            output = full.to_string();
        }
        if !s.state.is_live() {
            break;
        }
        assert!(Instant::now() < deadline, "timeout na execução do PHPUnit");
        std::thread::sleep(Duration::from_millis(30));
    }
    let tree = parser.finish();
    let s = manager.session(id).unwrap();
    (tree, s.state, s.exit_code, output)
}

#[test]
fn consolidates_config_and_bootstrap_from_project() {
    let root = Path::new(FIXTURE);
    // Detecta phpunit.xml e o bootstrap dele (vendor/autoload.php).
    let setup = consolidate(root, &None, &None);
    assert_eq!(setup.config, Some(root.join("phpunit.xml")));
    assert_eq!(setup.bootstrap, Some(root.join("vendor/autoload.php")));
    // Explícito vence: config e bootstrap fora do padrão.
    let setup = consolidate(
        root,
        &Some(PathBuf::from("phpunit.xml.dist")),
        &Some(PathBuf::from("vendor/autoload.php")),
    );
    assert_eq!(setup.config, Some(root.join("phpunit.xml.dist")));
    assert_eq!(setup.bootstrap, Some(root.join("vendor/autoload.php")));
    let _ = resolve_phpunit; // API pública usada pela UI
}

#[test]
fn full_suite_tree_has_durations_failures_and_locations() {
    let Some(php) = php() else {
        eprintln!("SKIP: php não disponível");
        return;
    };
    let Some(phar) = phpunit() else {
        eprintln!("SKIP: phpunit.phar não encontrado");
        return;
    };
    let root = Path::new(FIXTURE);
    let (tree, state, exit, output) = run_and_tree(root, cfg(TestScope::All), &ctx(&php, &phar));

    let stats = tree.stats();
    assert_eq!(stats.total, 7, "árvore completa: {stats:?}\n{output}");
    assert_eq!(stats.passed, 5);
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.skipped, 1);
    assert_eq!(exit, Some(1), "PHPUnit sai 1 com falhas");
    assert_eq!(state, SessionState::Finished);

    // Falha navegável: arquivo e linha do teste proposital (linha 41).
    let failures = tree.failures();
    assert_eq!(failures.len(), 1);
    let failure = failures[0];
    assert_eq!(failure.name, "testFalhaProposital");
    let (file, line) = failure.location.clone().expect("localização da falha");
    assert!(file.ends_with("CalculadoraTest.php"), "{file:?}");
    assert_eq!(line, 41, "linha do teste proposital");
    let message = failure.message.as_deref().unwrap_or("");
    assert!(message.contains("soma deveria falhar"), "{message}");

    // Data sets aparecem como nós nomeados dentro da suíte do método.
    let named = tree.find_test("testDivisao with data set \"positivos\"");
    assert!(named.is_some(), "data set nomeado na árvore");
    assert_eq!(named.unwrap().status, Some(TestStatus::Passed));
    let skipped = tree.find_test("testIgnorado").unwrap();
    assert_eq!(skipped.status, Some(TestStatus::Skipped));
    // Durações vêm de testFinished (mesmo 0ms é Some).
    let soma = tree.find_test("testSoma").unwrap();
    assert!(soma.duration.is_some());
}

#[test]
fn scopes_class_method_and_dataset_run_only_matching_tests() {
    let Some(php) = php() else {
        eprintln!("SKIP: php não disponível");
        return;
    };
    let Some(phar) = phpunit() else {
        eprintln!("SKIP: phpunit.phar não encontrado");
        return;
    };
    let root = Path::new(FIXTURE);
    let context = ctx(&php, &phar);

    // Classe: só os testes de UtilTest.
    let (tree, _, _, _) = run_and_tree(
        root,
        cfg(TestScope::Class {
            class: "UtilTest".into(),
            file: None,
        }),
        &context,
    );
    assert_eq!(tree.stats().total, 2, "só UtilTest: {:?}", tree.stats());
    assert_eq!(tree.stats().skipped, 1);

    // Método com provider: os dois data sets, sem testDivisaoPorZero.
    let (tree, _, _, _) = run_and_tree(
        root,
        cfg(TestScope::Method {
            class: "App\\Tests\\CalculadoraTest".into(),
            method: "testDivisao".into(),
        }),
        &context,
    );
    assert_eq!(
        tree.stats().total,
        2,
        "método + data sets: {:?}",
        tree.stats()
    );
    assert!(tree
        .find_test("testDivisao with data set \"negativos\"")
        .is_some());

    // Data set único: exatamente um nó.
    let (tree, _, _, _) = run_and_tree(
        root,
        cfg(TestScope::DataSet {
            class: "App\\Tests\\CalculadoraTest".into(),
            method: "testDivisao".into(),
            data_set: "positivos".into(),
        }),
        &context,
    );
    assert_eq!(tree.stats().total, 1, "um data set: {:?}", tree.stats());
    assert!(tree
        .find_test("testDivisao with data set \"positivos\"")
        .is_some());

    // Diretório: arquivo posicional executa só o que está nele.
    let (tree, _, _, _) = run_and_tree(
        root,
        cfg(TestScope::Directory {
            path: PathBuf::from("tests/UtilTest.php"),
        }),
        &context,
    );
    assert!(
        tree.stats().total == 2,
        "diretório/arquivo: {:?}",
        tree.stats()
    );
}

/// Cobertura REAL de ponta a ponta (T-RUN-TST-003-02): PHPUnit 11 + Xdebug
/// (modo coverage) geram o Clover e o importador disponibiliza por
/// arquivo/linha. Sem a extensão o teste se autopula (o importador tem
/// cobertura de contrato com fixture XML no schema real do PHPUnit 11).
#[test]
fn coverage_real_com_xdebug_por_arquivo_e_linha() {
    let Some(php) = php() else {
        eprintln!("SKIP: php não disponível");
        return;
    };
    let Some(phar) = phpunit() else {
        eprintln!("SKIP: phpunit.phar não encontrado");
        return;
    };
    let Some(so) = xdebug_so() else {
        eprintln!("SKIP: extensão Xdebug não encontrada (PHPIDE_XDEBUG_SO)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let clover = dir.path().join("clover.xml");
    // phpunit.xml + bootstrap consolidados (T-RUN-TST-002-01). A suite tem 1
    // falha proposital (exit 1), mas o Clover é escrito ao final mesmo assim.
    let out = Command::new(&php)
        .arg("-d")
        .arg(format!("zend_extension={}", so.display()))
        .arg("-d")
        .arg("xdebug.mode=coverage")
        .arg(&phar)
        .arg("-c")
        .arg("phpunit.xml")
        .arg("--coverage-clover")
        .arg(&clover)
        .current_dir(FIXTURE)
        .output()
        .expect("spawn phpunit com xdebug");
    assert!(
        clover.exists(),
        "Clover gerado pelo PHPUnit real\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let report = phpide_run::coverage::import_clover_file(&clover).expect("clover importado");
    let calc = report
        .file(Path::new(FIXTURE).join("src/Calculadora.php").as_path())
        .expect("Calculadora.php no relatório");
    assert!(calc.executable_lines() >= 2, "{calc:?}");
    assert!(calc.covered_lines() >= 2, "{calc:?}");
    assert!(calc.percent_tenths() > 0);
    // `return $a + $b;` (linha 9) executada pelos testes de soma.
    assert!(
        calc.line_hits(9).unwrap_or(0) > 0,
        "linha 9 coberta: {calc:?}"
    );
    // Agregado engloba Calculadora.php e Util.php.
    let (covered, executable) = report.totals();
    assert!(covered > 0, "totais: {covered}/{executable}");
    assert!(executable >= covered, "totais: {covered}/{executable}");
}
