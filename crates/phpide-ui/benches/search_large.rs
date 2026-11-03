//! Baseline de busca em projeto grande — T-NFR-006-02 (NFR-006).
//!
//! Gera um projeto fixture (300 arquivos PHP × 120 linhas — ver
//! `phpide_core::fixtures`), dispara a `SearchState` real da UI (worker do
//! `JobSystem`, canal, poll) e mede até a conclusão. Rodar:
//!
//! ```bash
//! cargo bench -p phpide-ui --bench search_large
//! ```

// O módulo de busca é incluído fora do binário: sem o consumer principal
// alguns itens ficam sem leitura neste contexto isolado.
#![allow(dead_code)]
// O `mod tests` do search.rs importa itens que só o binário principal usa.
#![allow(unused_imports)]

#[path = "../../phpide-core/benches/harness.rs"]
mod harness;

// A UI é um binário: incluímos o módulo de busca pelo caminho para medir
// o código real de produção.
#[path = "../src/search.rs"]
mod search;

use harness::bench;
use phpide_core::fixtures::write_large_project;
use phpide_core::jobs::JobSystem;
use search::SearchState;
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    let dir = std::env::temp_dir().join(format!("phpide-bench-proj-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // 300 arquivos × 120 linhas ≈ 36k linhas distribuídas.
    write_large_project(&dir, 300, 120).unwrap();
    let mut files: Vec<PathBuf> = Vec::new();
    for e in walk(&dir) {
        if search::is_searchable(&e) {
            files.push(e);
        }
    }
    // 300 arquivos .php + composer.json (também pesquisável) = 301.
    assert_eq!(files.len(), 301, "fixture completo");

    let sys = JobSystem::new();
    bench("search_across_300_files", 10, 2, || {
        let mut state = SearchState::new();
        state.query = "value_3".to_string();
        state.start(files.clone(), &sys);
        state.wait(Duration::from_secs(30));
        assert!(!state.busy(), "busca concluiu: {}", state.status);
    });

    std::fs::remove_dir_all(&dir).ok();
}

fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}
