//! PHPUnit — `T-RUN-002-03`, `T-RUN-TST-001-01/02`, `T-RUN-TST-002-01`.
//!
//! - **Resolução do runner** (TST-002-01): `PHPIDE_PHPUNIT` →
//!   `vendor/bin/phpunit` → `phpunit.phar` no projeto → PATH. O runner é
//!   sempre executado via interpretador PHP (`php <runner>`), o que cobre
//!   phar e o wrapper do vendor sem shebang garantido;
//! - **Consolidação config/bootstrap** (TST-002-01): configuração explícita
//!   vence; senão `phpunit.xml` → `phpunit.xml.dist` na raiz. Bootstrap
//!   explícito vence; senão o atributo `bootstrap=` do XML; senão
//!   `vendor/autoload.php` quando presente;
//! - **Escopos** (TST-001-01): diretório (argumento posicional), classe,
//!   método e data set (`--filter`), além de "tudo";
//! - **Árvore de resultados** (TST-001-02): parser incremental das
//!   mensagens TeamCity (`--teamcity`) — suítes aninhadas, duração por teste,
//!   falhas com mensagem/detalhes, stdout, e localização de arquivo:linha
//!   extraída dos detalhes da falha para navegação.

use std::path::{Path, PathBuf};
use std::time::Duration;

use phpide_core::{Component, ErrorKind, IdeError};
use phpide_exec::{discover, CommandSpec, ToolRequest};

use crate::config::{RunConfig, RunKind, TestScope};
use crate::php_script::BuildContext;

/// Resolução do runner PHPUnit (T-RUN-TST-002-01).
///
/// Ordem: variável de ambiente `PHPIDE_PHPUNIT` → `vendor/bin/phpunit` →
/// `phpunit.phar` na raiz do projeto → `phpunit` no PATH (NFR-003-01:
/// configuração explícita antes do PATH).
pub fn resolve_phpunit(root: &Path) -> Result<PathBuf, IdeError> {
    if let Ok(explicit) = std::env::var("PHPIDE_PHPUNIT") {
        if !explicit.is_empty() {
            let path = PathBuf::from(&explicit);
            if path.exists() {
                return Ok(path);
            }
            return Err(IdeError::new(
                Component::Exec,
                "resolve_phpunit",
                ErrorKind::NotFound,
                format!("PHPUNIT configurado não existe: {explicit}"),
                "corrija a variável PHPIDE_PHPUNIT",
            ));
        }
    }
    let vendor_bin = root.join("vendor/bin/phpunit");
    if vendor_bin.exists() {
        return Ok(vendor_bin);
    }
    let local_phar = root.join("phpunit.phar");
    if local_phar.exists() {
        return Ok(local_phar);
    }
    match discover(&ToolRequest::new("phpunit", "PHPUnit")) {
        Ok(info) => Ok(info.program),
        Err(_) => Err(IdeError::new(
            Component::Exec,
            "resolve_phpunit",
            ErrorKind::NotFound,
            "PHPUnit não encontrado",
            "instale com `composer require --dev phpunit/phpunit` ou baixe o phpunit.phar para a raiz do projeto",
        )),
    }
}

/// Configuração efetiva do PHPUnit (T-RUN-TST-002-01).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhpUnitSetup {
    /// Arquivo de configuração (phpunit.xml[.dist]) ou `None`.
    pub config: Option<PathBuf>,
    /// Bootstrap efetivo (autoload) ou `None`.
    pub bootstrap: Option<PathBuf>,
}

/// Consolida configuração e bootstrap do PHPUnit.
///
/// Explícito sempre vence; a detecção nunca sobrescreve configuração manual
/// (mesma regra do descobridor Composer, T-PHP-030-02).
pub fn consolidate(
    root: &Path,
    config: &Option<PathBuf>,
    bootstrap: &Option<PathBuf>,
) -> PhpUnitSetup {
    let config = config
        .clone()
        .map(|p| if p.is_absolute() { p } else { root.join(p) })
        .or_else(|| {
            ["phpunit.xml", "phpunit.xml.dist"]
                .iter()
                .map(|name| root.join(name))
                .find(|p| p.exists())
        });
    let bootstrap = bootstrap
        .clone()
        .map(|p| if p.is_absolute() { p } else { root.join(p) })
        .or_else(|| config.as_ref().and_then(|cfg| bootstrap_from_xml(cfg)))
        .or_else(|| {
            let autoload = root.join("vendor/autoload.php");
            autoload.exists().then_some(autoload)
        });
    PhpUnitSetup { config, bootstrap }
}

/// Lê o atributo `bootstrap` de um phpunit.xml (varredura de atributos,
/// sem crate XML — mesmo princípio do parser DBGp).
fn bootstrap_from_xml(path: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(path).ok()?;
    let start = text.find("<phpunit")?;
    let chunk = &text[start..text.len().min(start + 2000)];
    let end = chunk.find('>')?;
    let attrs = &chunk[..end];
    let needle = "bootstrap=\"";
    let bstart = attrs.find(needle)? + needle.len();
    let bend = attrs[bstart..].find('"')? + bstart;
    let value = &attrs[bstart..bend];
    let boot = PathBuf::from(value);
    // Relativo ao diretório do phpunit.xml (não ao cwd do processo).
    Some(if boot.is_absolute() {
        boot
    } else {
        path.parent()?.join(boot)
    })
}

/// Argumentos de linha de comando para o escopo (T-RUN-TST-001-01).
///
/// Diretório é posicional; classe/método/data set usam `--filter` (regex
/// sobre `Class::method with data set ...`).
pub fn scope_args(root: &Path, scope: &TestScope) -> Vec<String> {
    let abs = |p: &Path| {
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            root.join(p)
        }
    };
    match scope {
        TestScope::All => Vec::new(),
        TestScope::Directory { path } => vec![abs(path).to_string_lossy().into_owned()],
        TestScope::Class { class, file } => {
            let mut args = Vec::new();
            if let Some(f) = file {
                args.push(abs(f).to_string_lossy().into_owned());
            }
            args.push("--filter".into());
            args.push(class.clone());
            args
        }
        TestScope::Method { class, method } => {
            // Âncora no fim do método: `testDivisao( |$)` casa o método e
            // seus data sets, mas não `testDivisaoPorZero` (verificado com
            // PHPUnit 11 real). Backslashes do FQCN escapados para regex.
            vec![
                "--filter".into(),
                format!("{}::{}( |$)", regex_escape(class), regex_escape(method)),
            ]
        }
        TestScope::DataSet {
            class,
            method,
            data_set,
        } => {
            // Nome do data set vem citado (`with data set "positivos"`) ou
            // numérico (`with data set #0`); as `.?` casam as citações.
            vec![
                "--filter".into(),
                format!(
                    "{}::{} with data set .?{}.*$",
                    regex_escape(class),
                    regex_escape(method),
                    regex_escape(data_set)
                ),
            ]
        }
    }
}

/// Escapa para uso como literal dentro do regex do `--filter`.
fn regex_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
}

/// Plano de execução: o que a sessão precisa além do spec (ex.: cobertura).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhpUnitPlan {
    /// Arquivo Clover a importar ao final (`--coverage-clover`).
    pub clover_path: Option<PathBuf>,
}

/// Monta o `CommandSpec` do PHPUnit e o plano de artefatos.
pub fn build_command(
    config: &RunConfig,
    root: &Path,
    ctx: &BuildContext,
    cwd: PathBuf,
) -> Result<(CommandSpec, PhpUnitPlan), IdeError> {
    let RunKind::PhpUnit {
        scope,
        config: cfg,
        bootstrap,
        runner_options,
        coverage,
    } = &config.kind
    else {
        unreachable!("build_command de PHPUnit chamado com kind errado");
    };
    // Contexto vence a detecção (NFR-003-01: configuração explícita primeiro).
    let runner = match &ctx.phpunit {
        Some(path) => PathBuf::from(path),
        None => resolve_phpunit(root)?,
    };
    let setup = consolidate(root, cfg, bootstrap);

    let mut args = vec![runner.to_string_lossy().into_owned()];
    if let Some(cfg_path) = &setup.config {
        args.push("-c".into());
        args.push(cfg_path.to_string_lossy().into_owned());
    }
    if let Some(boot) = &setup.bootstrap {
        args.push("--bootstrap".into());
        args.push(boot.to_string_lossy().into_owned());
    }
    args.extend(scope_args(root, scope));
    args.extend(runner_options.iter().cloned());
    args.push("--teamcity".into());

    let mut plan = PhpUnitPlan::default();
    if *coverage {
        let clover = std::env::temp_dir().join(format!(
            "phpide-clover-{}-{}-{}.xml",
            std::process::id(),
            config.id,
            unique_n()
        ));
        args.push("--coverage-clover".into());
        args.push(clover.to_string_lossy().into_owned());
        plan.clover_path = Some(clover);
    }
    let spec = CommandSpec::new(
        config
            .interpreter
            .clone()
            .unwrap_or_else(|| ctx.php.clone()),
        args,
    )
    .cwd(cwd);
    Ok((spec, plan))
}

/// Contador simples para nomes únicos de relatório (sem depender de rand).
fn unique_n() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::SeqCst)
}

// ---------------------------------------------------------------------------
// Árvore de resultados (T-RUN-TST-001-02)
// ---------------------------------------------------------------------------

/// Status de um nó de teste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestStatus {
    /// Em execução (testStarted sem finished ainda).
    Running,
    Passed,
    Failed,
    Skipped,
    Incomplete,
}

impl TestStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            TestStatus::Running => "running",
            TestStatus::Passed => "passed",
            TestStatus::Failed => "failed",
            TestStatus::Skipped => "skipped",
            TestStatus::Incomplete => "incomplete",
        }
    }

    pub const fn is_failure(self) -> bool {
        matches!(self, TestStatus::Failed | TestStatus::Incomplete)
    }
}

/// Nó da árvore de testes: suíte ou teste.
#[derive(Debug, Clone, Default)]
pub struct TestNode {
    pub name: String,
    pub is_suite: bool,
    pub status: Option<TestStatus>,
    pub duration: Option<Duration>,
    pub message: Option<String>,
    pub details: Option<String>,
    pub location: Option<(PathBuf, usize)>,
    pub children: Vec<TestNode>,
}

impl TestNode {
    fn suite(name: impl Into<String>) -> Self {
        TestNode {
            name: name.into(),
            is_suite: true,
            ..Default::default()
        }
    }

    fn test(name: impl Into<String>) -> Self {
        TestNode {
            name: name.into(),
            is_suite: false,
            status: Some(TestStatus::Running),
            ..Default::default()
        }
    }

    /// Filho pelo nome (qualquer tipo de nó) — usado nos testes do parser.
    #[cfg(test)]
    fn child(&self, name: &str) -> Option<&TestNode> {
        self.children.iter().find(|c| c.name == name)
    }
}

/// Árvore de resultados com estatísticas.
#[derive(Debug, Clone, Default)]
pub struct TestTree {
    pub roots: Vec<TestNode>,
}

/// Contagem agregada da execução.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TestStats {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub incomplete: usize,
    pub duration: Duration,
}

impl TestTree {
    pub fn stats(&self) -> TestStats {
        let mut stats = TestStats::default();
        for root in &self.roots {
            accumulate(root, &mut stats);
        }
        stats
    }

    /// Nós de teste com falha, em ordem de descoberta (navegação à falha).
    pub fn failures(&self) -> Vec<&TestNode> {
        let mut out = Vec::new();
        for root in &self.roots {
            collect_failures(root, &mut out);
        }
        out
    }

    /// Procura um teste pelo nome exibido (ex.: "Classe::método with data set #1").
    pub fn find_test(&self, name: &str) -> Option<&TestNode> {
        fn walk<'a>(node: &'a TestNode, name: &str) -> Option<&'a TestNode> {
            if !node.is_suite && node.name == name {
                return Some(node);
            }
            node.children.iter().find_map(|c| walk(c, name))
        }
        self.roots.iter().find_map(|r| walk(r, name))
    }
}

fn accumulate(node: &TestNode, stats: &mut TestStats) {
    if node.is_suite {
        for c in &node.children {
            accumulate(c, stats);
        }
        return;
    }
    stats.total += 1;
    match node.status {
        Some(TestStatus::Passed) => stats.passed += 1,
        Some(TestStatus::Failed) => stats.failed += 1,
        Some(TestStatus::Skipped) => stats.skipped += 1,
        Some(TestStatus::Incomplete) => stats.incomplete += 1,
        _ => {}
    }
    if let Some(d) = node.duration {
        stats.duration += d;
    }
}

fn collect_failures<'a>(node: &'a TestNode, out: &mut Vec<&'a TestNode>) {
    if !node.is_suite && node.status.is_some_and(|s| s.is_failure()) {
        out.push(node);
    }
    for c in &node.children {
        collect_failures(c, out);
    }
}

// ---------------------------------------------------------------------------
// Parser TeamCity
// ---------------------------------------------------------------------------

/// Parser incremental das mensagens `--teamcity` do PHPUnit.
///
/// Mensagens: `testSuiteStarted/Finished`, `testStarted/Finished/Failed/
/// Ignored`, `testStdOut/testStdErr`. Escapes TeamCity (`|'`, `|[`, `|]`,
/// `||`) são revertidos. Linhas comuns (progresso textual) são ignoradas —
/// a árvore é a fonte de verdade (RUN-TST-001).
pub struct TeamCityParser {
    roots: Vec<TestNode>,
    /// Pilha de suítes abertas (caminho de índices na árvore).
    suite_path: Vec<usize>,
    /// Pilha de testes abertos (nome, caminho de índices).
    test_path: Vec<(String, Vec<usize>)>,
}

impl Default for TeamCityParser {
    fn default() -> Self {
        Self::new()
    }
}

impl TeamCityParser {
    pub fn new() -> Self {
        TeamCityParser {
            roots: Vec::new(),
            suite_path: Vec::new(),
            test_path: Vec::new(),
        }
    }

    /// Consome uma linha de stdout do processo PHPUnit.
    pub fn feed_line(&mut self, line: &str) {
        let Some(msg) = line.trim_start().strip_prefix("##teamcity[") else {
            return;
        };
        let Some(end) = msg.rfind(']') else { return };
        let msg = &msg[..end];
        let (name, attrs) = match msg.split_once([' ', '\t']) {
            Some((n, rest)) => (n, rest),
            None => (msg, ""),
        };
        let attrs = parse_attrs(attrs);
        let attr = |key: &str| attrs.get(key).map(String::as_str);
        match name {
            "testSuiteStarted" => {
                let suite = TestNode::suite(attr("name").unwrap_or("?"));
                // O caminho retornado é a pilha completa da nova suíte.
                self.suite_path = self.insert_node(suite);
            }
            "testSuiteFinished" => {
                self.suite_path.pop();
            }
            "testStarted" => {
                let mut node = TestNode::test(attr("name").unwrap_or("?"));
                // Localização vem do locationHint: php_qn://<arquivo>::<fqcn>.
                node.location = attr("locationHint").and_then(location_hint_path);
                let idx = self.insert_node(node);
                self.test_path
                    .push((attr("name").unwrap_or("?").to_string(), idx));
            }
            "testFinished" => {
                let duration = attr("duration").and_then(|d| d.parse::<u64>().ok());
                if let Some(node) = self.open_test_mut(attr("name")) {
                    // testFinished sempre segue o teste; testFailed/
                    // testIgnored (que vêm antes) têm precedência.
                    if node.status == Some(TestStatus::Running) {
                        node.status = Some(TestStatus::Passed);
                    }
                    node.duration = duration.map(Duration::from_millis);
                }
            }
            "testFailed" => {
                if let Some(node) = self.open_test_mut(attr("name")) {
                    node.status = Some(TestStatus::Failed);
                    node.message = attr("message").map(str::to_string);
                    node.details = attr("details").map(str::to_string);
                    // Linha vem dos detalhes ("/arquivo.php:41"); o arquivo
                    // já veio do locationHint no testStarted.
                    if let Some((file, line)) = node.details.as_deref().and_then(extract_location) {
                        node.location = Some(match node.location.take() {
                            Some((hint_file, _)) => (hint_file, line),
                            None => (file, line),
                        });
                    }
                }
            }
            "testIgnored" => {
                if let Some(node) = self.open_test_mut(attr("name")) {
                    node.status = Some(TestStatus::Skipped);
                    node.message = attr("message").map(str::to_string);
                }
            }
            _ => {}
        }
    }

    /// Finaliza: testes ainda abertos contam como passados sem duração
    /// (saída abrupta do runner) e suítes abertas são fechadas.
    pub fn finish(mut self) -> TestTree {
        for (_, path) in self.test_path.drain(..) {
            if let Some(node) = node_at_mut(&mut self.roots, &path) {
                if node.status == Some(TestStatus::Running) {
                    node.status = Some(TestStatus::Passed);
                }
            }
        }
        TestTree { roots: self.roots }
    }

    fn insert_node(&mut self, node: TestNode) -> Vec<usize> {
        if !self.suite_path.is_empty() {
            let mut path = self.suite_path.clone();
            let parent = node_at_mut(&mut self.roots, &path).expect("suite aberta existe");
            parent.children.push(node);
            path.push(parent.children.len() - 1);
            path
        } else {
            self.roots.push(node);
            vec![self.roots.len() - 1]
        }
    }

    fn open_test_mut(&mut self, name: Option<&str>) -> Option<&mut TestNode> {
        let name = name?;
        // Casamento por nome, do mais recente para o mais antigo.
        let pos = self.test_path.iter().rposition(|(n, _)| n == name)?;
        let path = self.test_path[pos].1.clone();
        let node = node_at_mut(&mut self.roots, &path)?;
        Some(node)
    }
}

fn node_at_mut<'a>(roots: &'a mut [TestNode], path: &[usize]) -> Option<&'a mut TestNode> {
    let (first, rest) = path.split_first()?;
    let mut node = roots.get_mut(*first)?;
    for idx in rest {
        node = node.children.get_mut(*idx)?;
    }
    Some(node)
}

/// Caminho do arquivo no locationHint: `php_qn:///path/File.php::\Fqcn::m`.
fn location_hint_path(hint: &str) -> Option<(PathBuf, usize)> {
    let rest = hint.strip_prefix("php_qn://")?;
    let file = rest.split("::").next()?;
    if file.is_empty() {
        return None;
    }
    Some((PathBuf::from(file), 0))
}

/// Extrai `arquivo.php:linha` do stack trace nos detalhes da falha.
fn extract_location(details: &str) -> Option<(PathBuf, usize)> {
    // Formato típico: "/path/Test.php(23): Classe->metodo()" ou
    // "Failed asserting that ... in /path/Test.php on line 23" (HTML) ou
    // PHPUnit 11: "1) Classe::teste\n/path/Test.php:23\n..."
    for line in details.lines() {
        let bytes = line.as_bytes();
        let mut start = None;
        for (i, b) in bytes.iter().enumerate() {
            if *b == b'/' {
                start = Some(i);
                break;
            }
        }
        let Some(start) = start else { continue };
        let rest = &line[start..];
        if let Some(paren) = rest.find("(") {
            if rest[paren..].starts_with("(") {
                if let Some(close) = rest[paren..].find(')') {
                    if let Ok(lineno) = rest[paren + 1..paren + close].parse::<usize>() {
                        let file = PathBuf::from(&rest[..paren]);
                        if file.extension().is_some_and(|e| e == "php") {
                            return Some((file, lineno));
                        }
                    }
                }
            }
        }
        if let Some(colon) = rest.rfind(':') {
            if let Ok(lineno) = rest[colon + 1..].trim().parse::<usize>() {
                let file = PathBuf::from(rest[..colon].trim());
                if file.extension().is_some_and(|e| e == "php") {
                    return Some((file, lineno));
                }
            }
        }
    }
    None
}

/// Parse de atributos TeamCity: `name='valor' message='m'`.
fn parse_attrs(input: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let mut rest = input;
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim();
        let after = &rest[eq + 1..];
        let Some(stripped) = after.strip_prefix('\'') else {
            break;
        };
        let mut value = String::new();
        let mut chars = stripped.char_indices();
        let mut end = stripped.len();
        while let Some((i, c)) = chars.next() {
            if c == '|' {
                if let Some((_, next)) = chars.next() {
                    match next {
                        '\'' | '[' | ']' | '|' => value.push(next),
                        'n' => value.push('\n'),
                        'r' => value.push('\r'),
                        other => {
                            value.push('|');
                            value.push(other);
                        }
                    }
                }
                continue;
            }
            if c == '\'' {
                end = i;
                break;
            }
            value.push(c);
        }
        if !key.is_empty() {
            out.insert(key.to_string(), value);
        }
        // Avança além da aspa de fechamento para o próximo atributo.
        rest = stripped.get(end + 1..).unwrap_or("");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Linhas exatamente como o PHPUnit 11 emite (backslash simples nos FQCN).
    const SESSION: &str = r##"
##teamcity[testCount count='3']
##teamcity[testSuiteStarted name='projeto' flowId='1']
##teamcity[testSuiteStarted name='App\Tests\CalcTest' locationHint='php_qn:///src/CalcTest.php::\App\Tests\CalcTest' flowId='1']
##teamcity[testStarted name='testOk' locationHint='php_qn:///src/CalcTest.php::\App\Tests\CalcTest::testOk' flowId='1']
##teamcity[testFinished name='testOk' duration='12' flowId='1']
##teamcity[testStarted name='testFalha' locationHint='php_qn:///src/CalcTest.php::\App\Tests\CalcTest::testFalha' flowId='1']
##teamcity[testFailed name='testFalha' message='quebrou|nsegunda linha' details='/src/CalcTest.php:41|n' duration='3' flowId='1']
##teamcity[testFinished name='testFalha' duration='3' flowId='1']
##teamcity[testSuiteFinished name='App\Tests\CalcTest' flowId='1']
##teamcity[testSuiteFinished name='projeto' flowId='1']
"##;

    #[test]
    fn parses_suites_tests_durations_and_failure_location() {
        let mut parser = TeamCityParser::new();
        for line in SESSION.lines() {
            parser.feed_line(line);
        }
        let tree = parser.finish();
        assert_eq!(tree.roots.len(), 1);
        let project = &tree.roots[0];
        assert!(project.is_suite);
        let class_suite = project.child("App\\Tests\\CalcTest").unwrap();
        assert_eq!(class_suite.children.len(), 2);

        let stats = tree.stats();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.passed, 1);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.duration, Duration::from_millis(15));

        let failures = tree.failures();
        assert_eq!(failures.len(), 1);
        let f = failures[0];
        assert_eq!(f.name, "testFalha");
        assert_eq!(f.location, Some((PathBuf::from("/src/CalcTest.php"), 41)));
        // Escape |n vira quebra de linha real.
        assert_eq!(f.message.as_deref(), Some("quebrou\nsegunda linha"));
        let ok = tree.find_test("testOk").unwrap();
        assert_eq!(ok.status, Some(TestStatus::Passed));
        assert_eq!(ok.duration, Some(Duration::from_millis(12)));
    }

    #[test]
    fn data_provider_emits_method_suite_with_named_children() {
        let mut parser = TeamCityParser::new();
        for line in [
            r#"##teamcity[testSuiteStarted name='App\Tests\CalcTest']"#,
            r#"##teamcity[testSuiteStarted name='testDivisao' locationHint='php_qn:///t.php::\App\Tests\CalcTest::testDivisao']"#,
            r#"##teamcity[testStarted name='testDivisao with data set "positivos"' locationHint='php_qn:///t.php::\App\Tests\CalcTest::testDivisao with data set "positivos"']"#,
            r#"##teamcity[testFinished name='testDivisao with data set "positivos"' duration='2']"#,
            r#"##teamcity[testSuiteFinished name='testDivisao']"#,
            r#"##teamcity[testSuiteFinished name='App\Tests\CalcTest']"#,
        ] {
            parser.feed_line(line);
        }
        let tree = parser.finish();
        let node = tree
            .find_test("testDivisao with data set \"positivos\"")
            .unwrap();
        assert_eq!(node.status, Some(TestStatus::Passed));
        assert_eq!(node.location, Some((PathBuf::from("/t.php"), 0)));
        // Suítes aninhadas somam zero no total de testes.
        assert_eq!(tree.stats().total, 1);
    }

    #[test]
    fn ignored_tests_mark_skipped_and_unfinished_become_passed() {
        let mut parser = TeamCityParser::new();
        for line in [
            "##teamcity[testStarted name='pulado']",
            "##teamcity[testIgnored name='pulado' message='sem motivo']",
            "##teamcity[testStarted name='abandonado']",
        ] {
            parser.feed_line(line);
        }
        let tree = parser.finish();
        assert_eq!(
            tree.find_test("pulado").unwrap().status,
            Some(TestStatus::Skipped)
        );
        assert_eq!(
            tree.find_test("abandonado").unwrap().status,
            Some(TestStatus::Passed)
        );
    }

    #[test]
    fn non_teamcity_lines_are_ignored() {
        let mut parser = TeamCityParser::new();
        parser.feed_line("PHPUnit 11.5 by Sebastian Bergmann and contributors.");
        parser.feed_line(
            "..F                                                                   3 / 3 (100%)",
        );
        parser.feed_line("##teamcity[testStarted name='t']");
        parser.feed_line("##teamcity[testFinished name='t' duration='1']");
        let tree = parser.finish();
        assert_eq!(tree.stats().total, 1);
    }

    #[test]
    fn scope_args_cover_all_forms() {
        let root = Path::new("/proj");
        assert_eq!(scope_args(root, &TestScope::All), Vec::<String>::new());
        assert_eq!(
            scope_args(
                root,
                &TestScope::Directory {
                    path: PathBuf::from("tests")
                }
            ),
            vec!["/proj/tests".to_string()]
        );
        assert_eq!(
            scope_args(
                root,
                &TestScope::Method {
                    class: "App\\Tests\\C".into(),
                    method: "m".into()
                }
            ),
            vec![
                "--filter".to_string(),
                "App\\\\Tests\\\\C::m( |$)".to_string()
            ]
        );
        assert_eq!(
            scope_args(
                root,
                &TestScope::DataSet {
                    class: "C".into(),
                    method: "m".into(),
                    data_set: "#0".into()
                }
            ),
            vec![
                "--filter".to_string(),
                "C::m with data set .?#0.*$".to_string()
            ]
        );
    }

    #[test]
    fn bootstrap_from_xml_resolves_relative_to_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        let xml = dir.path().join("phpunit.xml");
        std::fs::write(&xml, "<phpunit bootstrap=\"../autoload.php\"></phpunit>").unwrap();
        let setup = consolidate(dir.path(), &None, &None);
        assert_eq!(setup.config, Some(xml.clone()));
        assert_eq!(setup.bootstrap, Some(dir.path().join("../autoload.php")));
        // Sem bootstrap no XML nem vendor → None (PHPUnit roda sem bootstrap).
        std::fs::write(&xml, "<phpunit></phpunit>").unwrap();
        let setup = consolidate(dir.path(), &None, &None);
        assert_eq!(setup.bootstrap, None);
    }
}
