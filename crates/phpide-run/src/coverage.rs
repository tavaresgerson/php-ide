//! Cobertura de código — `T-RUN-TST-003-01/02` (RUN-TST-003).
//!
//! Modelo por **arquivo e linha executável**: uma linha só existe no mapa
//! quando o relatório a lista — ausência significa "não executável" e nunca
//! é confundida com sucesso (RUN-TST-003: "coverage não é sucesso de teste").
//! Importador do formato **Clover XML** (`--coverage-clover`), gerado pelo
//! PHPUnit com driver Xdebug ou PCOV. Parsing por varredura de atributos
//! (sem crate XML), tolerante a atributos em qualquer ordem.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use phpide_core::{Component, ErrorKind, IdeError};

/// Cobertura de um arquivo: mapa linha → número de execuções.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileCoverage {
    /// Linha executável → vezes que foi executada (0 = executável, não coberta).
    pub lines: BTreeMap<u32, u64>,
}

impl FileCoverage {
    /// Linhas executáveis listadas no relatório.
    pub fn executable_lines(&self) -> usize {
        self.lines.len()
    }

    /// Linhas executadas ao menos uma vez.
    pub fn covered_lines(&self) -> usize {
        self.lines.values().filter(|hits| **hits > 0).count()
    }

    /// Percentual 0..=100 com uma casa decimal (u32 em décimos: 0..=1000).
    pub fn percent_tenths(&self) -> u32 {
        let exec = self.executable_lines();
        if exec == 0 {
            return 0;
        }
        (self.covered_lines() * 1000 / exec) as u32
    }

    /// `Some(hits)` se a linha for executável; `None` = desconhecida/não executável.
    pub fn line_hits(&self, line: u32) -> Option<u64> {
        self.lines.get(&line).copied()
    }
}

/// Relatório de cobertura de uma execução.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoverageReport {
    /// Arquivo (caminho absoluto como veio no relatório) → cobertura.
    pub files: BTreeMap<PathBuf, FileCoverage>,
}

impl CoverageReport {
    /// Totais agregados (executáveis, cobertos) de todos os arquivos.
    pub fn totals(&self) -> (usize, usize) {
        let exec: usize = self
            .files
            .values()
            .map(FileCoverage::executable_lines)
            .sum();
        let covered: usize = self.files.values().map(FileCoverage::covered_lines).sum();
        (exec, covered)
    }

    /// Percentual agregado (décimos).
    pub fn percent_tenths(&self) -> u32 {
        let exec: usize = self
            .files
            .values()
            .map(FileCoverage::executable_lines)
            .sum();
        if exec == 0 {
            return 0;
        }
        let covered: usize = self.files.values().map(FileCoverage::covered_lines).sum();
        (covered * 1000 / exec) as u32
    }

    /// Lookup por sufixo de caminho (o relatório pode conter caminhos
    /// absolutos do container/CI; a UI normaliza antes via `normalize`).
    pub fn file(&self, path: &Path) -> Option<&FileCoverage> {
        self.files.get(path).or_else(|| {
            let needle = path.to_string_lossy();
            self.files
                .iter()
                .find(|(k, _)| k.to_string_lossy().ends_with(needle.as_ref()))
                .map(|(_, v)| v)
        })
    }
}

/// Importa Clover XML (`<coverage><project>...<file name=...><line num count>`).
///
/// Tolerante a namespaces e ordem de atributos; `line` sem `count` conta
/// como 0. Erro de XML malformado vira `IdeError` recuperável.
pub fn import_clover(xml: &str) -> Result<CoverageReport, IdeError> {
    if !xml.contains("<coverage") || !xml.contains("</coverage>") || !xml.contains("<file ") {
        return Err(IdeError::new(
            Component::Core,
            "import_clover",
            ErrorKind::InvalidInput,
            "arquivo Clover inválido ou vazio",
            "gere o relatório com --coverage-clover do PHPUnit",
        )
        .with_detail("marcadores <coverage>/<file> ausentes"));
    }
    let mut report = CoverageReport::default();
    let mut rest = xml;
    while let Some(file_start) = rest.find("<file ") {
        rest = &rest[file_start..];
        let tag_end = rest.find('>').ok_or_else(malformed)?;
        let attrs = &rest[..tag_end];
        let name = xml_attr(attrs, "name").ok_or_else(malformed)?;
        let close = rest.find("</file>").ok_or_else(malformed)?;
        let body = &rest[..close];
        rest = &rest[close + "</file>".len()..];

        let mut coverage = FileCoverage::default();
        let mut lines = body;
        while let Some(line_start) = lines.find("<line ") {
            lines = &lines[line_start..];
            let end = lines.find('>').ok_or_else(malformed)?;
            let attrs = &lines[..end];
            let num = xml_attr(attrs, "num").and_then(|n| n.parse::<u32>().ok());
            let count = xml_attr(attrs, "count").and_then(|c| c.parse::<u64>().ok());
            if let Some(num) = num {
                coverage.lines.insert(num, count.unwrap_or(0));
            }
            lines = &lines[end..];
        }
        report.files.insert(PathBuf::from(name), coverage);
    }
    Ok(report)
}

/// Importa direto de um arquivo Clover.
pub fn import_clover_file(path: &Path) -> Result<CoverageReport, IdeError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        IdeError::new(
            Component::Core,
            "import_clover_file",
            ErrorKind::Io,
            format!("não foi possível ler o relatório: {}", path.display()),
            "execute os testes com cobertura habilitada",
        )
        .with_detail(e.to_string())
    })?;
    import_clover(&text)
}

fn malformed() -> IdeError {
    IdeError::new(
        Component::Core,
        "import_clover",
        ErrorKind::InvalidInput,
        "Clover XML malformado",
        "regenere o relatório de cobertura",
    )
}

/// Extrai valor de atributo: `num="12"`.
fn xml_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let rel_end = tag[start..].find('"')?;
    Some(&tag[start..start + rel_end])
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLOVER: &str = r#"<?xml version="1.0"?>
<coverage generated="1730000000">
  <project timestamp="1730000000">
    <file name="/proj/src/Calculadora.php">
      <line num="5" type="stmt" count="3"/>
      <line num="6" type="stmt" count="0"/>
      <line num="9" type="method" count="1" crap="1"/>
      <line num="10" type="stmt" count="2"/>
      <metrics loc="20" ncloc="15" statements="3" coveredstatements="2"/>
    </file>
    <file name="/proj/src/Util.php">
      <line num="3" type="stmt" count="1"/>
      <line num="4" type="stmt" count="1"/>
      <metrics loc="5" ncloc="5" statements="2" coveredstatements="2"/>
    </file>
  </project>
</coverage>"#;

    #[test]
    fn imports_file_line_hits() {
        let report = import_clover(CLOVER).unwrap();
        assert_eq!(report.files.len(), 2);
        let calc = report.file(Path::new("/proj/src/Calculadora.php")).unwrap();
        assert_eq!(calc.executable_lines(), 4);
        assert_eq!(calc.covered_lines(), 3);
        assert_eq!(calc.line_hits(6), Some(0), "linha executável não coberta");
        assert_eq!(calc.line_hits(99), None, "linha inexistente = desconhecida");
        // 3/4 → 75.0%
        assert_eq!(calc.percent_tenths(), 750);
    }

    #[test]
    fn aggregates_totals() {
        let report = import_clover(CLOVER).unwrap();
        // 6 linhas executáveis no total; Calculadora.php tem a linha 6 sem
        // execuções → 5/6 cobertas = 83.3%.
        assert_eq!(report.percent_tenths(), 833);
        assert_eq!(report.totals(), (6, 5));
        let util = report.file(Path::new("src/Util.php")).unwrap();
        assert_eq!(util.percent_tenths(), 1000, "lookup por sufixo");
    }

    #[test]
    fn missing_count_counts_as_zero() {
        let xml = r#"<coverage><project><file name="a.php"><line num="2" type="stmt"/></file></project></coverage>"#;
        let report = import_clover(xml).unwrap();
        assert_eq!(
            report.file(Path::new("a.php")).unwrap().line_hits(2),
            Some(0)
        );
    }

    #[test]
    fn invalid_input_is_recoverable_error() {
        let err = import_clover("<html>não é clover</html>").unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert!(
            import_clover("<coverage><file name=\"a\"><line num=\"1\" count=\"1\"/></file>")
                .is_err()
        );
    }

    #[test]
    fn empty_report_has_zero_percent_not_panic() {
        let report = CoverageReport::default();
        assert_eq!(report.percent_tenths(), 0);
        assert_eq!(report.totals(), (0, 0));
    }
}
