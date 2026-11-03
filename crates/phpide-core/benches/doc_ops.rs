//! Baseline de desempenho do núcleo de documentos — T-NFR-006-02 (NFR-006).
//!
//! Mede sobre o fixture de 10 mil linhas (`phpide_core::fixtures`):
//! digitação (insert no fim e no meio), conversões de posição (linha/col
//! e UTF-16), abertura de buffer e save atômico. Rodar:
//!
//! ```bash
//! cargo bench -p phpide-core --bench doc_ops
//! ```

#[path = "harness.rs"]
mod harness;

use harness::bench;
use phpide_core::fixtures::large_php_document;
use phpide_core::{Buffer, Document, PositionEncoding};
use std::path::PathBuf;

fn main() {
    let text_10k = large_php_document(10_000);
    let dir = std::env::temp_dir().join(format!("phpide-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("big10k.php");
    std::fs::write(&file, &text_10k).unwrap();

    // Digitação: um caractere por vez no FIM do documento (10k linhas).
    bench("typing_insert_end_10k", 200, 20, || {
        let mut doc = Document::new(&text_10k);
        for _ in 0..50 {
            doc.insert(doc.len_chars(), "x");
        }
    });

    // Digitação no MEIO do documento (pior caso da rope).
    let mid = text_10k.chars().count() / 2;
    bench("typing_insert_middle_10k", 200, 20, || {
        let mut doc = Document::new(&text_10k);
        for _ in 0..50 {
            doc.insert(mid, "x");
        }
    });

    // line_col (UTF-32/char) em offsets aleatórios determinísticos.
    let doc = Document::new(&text_10k);
    let len = doc.len_chars();
    let offsets: Vec<usize> = (0..100).map(|i| (i * 7919) % len).collect();
    bench("line_col_random_100_offsets", 50, 10, || {
        for &o in &offsets {
            let _ = doc.line_col_encoded(o, PositionEncoding::Utf32);
        }
    });

    // Conversões UTF-16 (LSP) ida e volta.
    bench("utf16_roundtrip_100_offsets", 50, 10, || {
        for &o in &offsets {
            let (l, c) = doc.line_col_encoded(o, PositionEncoding::Utf16);
            let _ = doc.lsp_position_to_char(l, c, PositionEncoding::Utf16);
        }
    });

    // Abertura de buffer de 10k linhas (leitura + rope).
    bench("buffer_open_10k", 50, 10, || {
        let _ = Buffer::open(&file).unwrap();
    });

    // Save atômico de 10k linhas (tmp + fsync + rename).
    let mut buf = Buffer::open(&file).unwrap();
    buf.insert(buf.document.len_chars(), "// edição\n");
    bench("buffer_save_atomic_10k", 50, 10, || {
        buf.save().unwrap();
    });

    std::fs::remove_dir_all(&dir).ok();
    let _ = PathBuf::from(&file);
}
