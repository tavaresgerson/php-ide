//! Geradores de massa de dados para testes e benchmarks — `T-NFR-006-01` (NFR-006).
//!
//! Massas usadas pela pirâmide de testes não funcionais:
//!
//! - [`large_php_document`]: arquivo PHP sintaticamente válido com N linhas
//!   (meta NFR-006: 10 mil linhas navegáveis);
//! - [`write_large_project`]: projeto com M arquivos PHP (busca/indexação);
//! - [`large_resultset_csv`]: resultset grande em CSV (preparação do DB).
//!
//! A geração é determinística (sem timestamps/aleatoriedade) para que
//! benchmarks sejam reproduzíveis. Ver `docs/baseline/T-NFR-006-01-fixtures.md`.

use std::fs;
use std::io;
use std::path::Path;

/// Arquivo PHP grande e sintaticamente plausível com `lines` linhas.
/// Estrutura: `<?php` + namespaces/classes/métodos aninhados + corpo.
pub fn large_php_document(lines: usize) -> String {
    let mut out = String::with_capacity(lines * 24);
    out.push_str("<?php\n");
    out.push_str("declare(strict_types=1);\n\n");
    out.push_str("namespace App\\Generated;\n\n");
    out.push_str("final class BigGenerated\n{\n");
    let mut remaining = lines.saturating_sub(4);
    let mut method = 0usize;
    while remaining > 0 {
        out.push_str(&format!(
            "    public function method_{method:05}(int $seed): int\n    {{\n"
        ));
        remaining = remaining.saturating_sub(2);
        let body = remaining.min(8);
        for i in 0..body {
            out.push_str(&format!(
                "        $value_{i} = ($seed + {i}) * 2 + $seed;\n"
            ));
        }
        remaining = remaining.saturating_sub(body);
        out.push_str("        return $seed + 1;\n    }\n\n");
        remaining = remaining.saturating_sub(2);
        method += 1;
        if remaining < 6 {
            break;
        }
    }
    out.push_str("}\n");
    out
}

/// Grava um projeto grande em `dir`: `files` arquivos `.php` com
/// `lines_per_file` linhas cada, mais um `composer.json` mínimo.
pub fn write_large_project(dir: &Path, files: usize, lines_per_file: usize) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::create_dir_all(dir.join("src"))?;
    fs::write(
        dir.join("composer.json"),
        r#"{"name": "fixture/large", "autoload": {"psr-4": {"App\\": "src/"}}}"#,
    )?;
    for i in 0..files {
        let sub = dir.join("src").join(format!("mod_{:03}", i % 16));
        fs::create_dir_all(&sub)?;
        let content = large_php_document(lines_per_file).replace(
            "namespace App\\Generated;",
            &format!("namespace App\\Mod{i:03};"),
        );
        fs::write(sub.join(format!("file_{i:04}.php")), content)?;
    }
    Ok(())
}

/// Resultset grande em CSV (header + `rows` linhas) para o futuro RF-DB:
/// valores nulos, acentos e strings longos misturados.
pub fn large_resultset_csv(rows: usize) -> String {
    let mut out = String::with_capacity(rows * 40);
    out.push_str("id,nome,email,saldo,observacao\n");
    for i in 0..rows {
        let obs = if i % 97 == 0 {
            ""
        } else {
            "observação com acentuação e texto suficientemente longo para ocupar espaço"
        };
        let saldo = format!("{:.2}", (i % 1000) as f64 * 1.25);
        out.push_str(&format!(
            "{i},Cliente {i:07},cliente{i}@exemplo.invalid,{saldo},{obs}\n"
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_document_has_at_least_requested_lines() {
        let text = large_php_document(10_000);
        assert!(
            text.lines().count() >= 10_000,
            "meta NFR-006: 10 mil linhas"
        );
        assert!(text.starts_with("<?php"));
        assert!(text.contains("namespace App\\Generated;"));
    }

    #[test]
    fn large_document_is_deterministic() {
        assert_eq!(large_php_document(500), large_php_document(500));
    }

    #[test]
    fn large_project_generates_expected_files() {
        let dir = std::env::temp_dir().join(format!("phpide-fixproj-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        write_large_project(&dir, 40, 120).unwrap();
        let count = fs::read_dir(dir.join("src"))
            .unwrap()
            .flat_map(|d| fs::read_dir(d.unwrap().path()))
            .flatten()
            .count();
        assert_eq!(count, 40);
        assert!(dir.join("composer.json").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resultset_csv_shape() {
        let csv = large_resultset_csv(1_000);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 1001, "header + 1000 linhas");
        // lines[0] é o header; o registro i=500 ocupa lines[501].
        assert!(lines[501].starts_with("500,Cliente 0000500,"));
        // Registro com observação vazia (i=97) termina com campo vazio: ",".
        assert!(lines[98].starts_with("97,Cliente 0000097,"));
        assert!(lines[98].ends_with(','));
    }
}
