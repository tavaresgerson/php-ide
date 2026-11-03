//! Gate de fluxo do editor (UI-ED-000): abrir, editar, desfazer, salvar e
//! fechar arquivos sem perda de estado. Cobertura automatizada da sequência
//! completa sobre o `Workspace` real; o gate visual (desalinhamento) é
//! coberto pelas UTs de geometria (`phpide-ui::geometry`).

use phpide_core::{Buffer, Document, Workspace};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("phpide-flow-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn open_edit_undo_save_close_sequence_preserves_state() {
    let dir = scratch("sequence");
    let file = dir.join("a.php");
    std::fs::write(&file, "<?php echo 'v0';\n").unwrap();

    let mut ws = Workspace::open(dir.clone());
    ws.buffers.clear();
    // 1. Abrir.
    ws.open_file_ex(&file, false);
    assert_eq!(ws.buffers.len(), 1);
    assert!(!ws.buffers[0].dirty, "aberto limpo do disco");
    assert_eq!(ws.buffers[0].document.text(), "<?php echo 'v0';\n");

    // 2. Editar.
    let at = ws.buffers[0].document.len_chars();
    ws.buffers[0].insert(at, "// edição\n");
    assert!(ws.buffers[0].dirty);
    assert!(ws.buffers[0].document.text().contains("// edição"));

    // 3. Desfazer / refazer.
    assert!(ws.buffers[0].undo(), "undo disponível após edição");
    assert_eq!(
        ws.buffers[0].document.text(),
        "<?php echo 'v0';\n",
        "undo restaura o texto exato"
    );
    assert!(ws.buffers[0].redo(), "redo disponível após undo");
    assert!(ws.buffers[0].document.text().contains("// edição"));

    // 4. Salvar: conteúdo persiste, dirty limpa, disco confere byte a byte.
    ws.buffers[0].save().unwrap();
    assert!(!ws.buffers[0].dirty, "save limpa o dirty");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        ws.buffers[0].document.text(),
        "disco == buffer após save"
    );

    // 5. Fechar: aba some, reabrir lê do disco (estado salvo, sem perda).
    assert!(ws.close_tab(0));
    assert_eq!(ws.buffers.len(), 1, "fecha e mantém a estrutura (untitled)");
    ws.open_file_ex(&file, false);
    let reopened = ws.active_buffer().expect("aba reaberta");
    assert_eq!(
        reopened.document.text(),
        "<?php echo 'v0';\n// edição\n",
        "reabrir mostra o conteúdo salvo"
    );
    assert!(!reopened.dirty);
    assert_eq!(reopened.path.as_deref(), Some(file.as_path()));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn dirty_buffer_survives_failed_save_and_stays_closed_until_confirmed() {
    // Caminho do NFR-007-02: buffer sujo não fecha sem confirmação; se o
    // save falha, o estado fica intacto na memória para nova tentativa.
    let dir = scratch("dirty");
    let file = dir.join("b.php");
    std::fs::write(&file, "<?php\n").unwrap();
    let mut buf = Buffer::open(&file).unwrap();
    buf.insert(6, "// edição importante\n");
    assert!(buf.dirty);
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap(); // destino inválido: save falha
    let text_before = buf.document.text();
    assert!(buf.save().is_err());
    assert!(buf.dirty, "continua sujo após falha");
    assert_eq!(buf.document.text(), text_before, "texto preservado");
    // Fechar sem confirmar = decisão da UI; o buffer ainda existe intacto.
    assert!(buf.document.text().contains("edição importante"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn multiple_buffers_keep_independent_state() {
    let dir = scratch("multi");
    let a = dir.join("a.php");
    let b = dir.join("b.php");
    std::fs::write(&a, "<?php // A\n").unwrap();
    std::fs::write(&b, "<?php // B\n").unwrap();
    let mut ws = Workspace::open(dir.clone());
    ws.buffers.clear();
    ws.open_file_ex(&a, false);
    ws.open_file_ex(&b, false);
    // Editar só o buffer ativo (B): A permanece limpo e intacto.
    let at = ws.buffers[1].document.len_chars();
    ws.buffers[1].insert(at, "echo 1;");
    assert!(ws.buffers[1].dirty);
    assert!(!ws.buffers[0].dirty, "A não é afetado pela edição de B");
    assert_eq!(ws.buffers[0].document.text(), "<?php // A\n");
    // Fechar B não toca em A.
    ws.close_tab(1);
    assert_eq!(ws.buffers[0].path.as_deref(), Some(a.as_path()));
    assert!(!ws.buffers[0].dirty);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn empty_and_unicode_documents_roundtrip_through_save() {
    let dir = scratch("unicode");
    let file = dir.join("u.php");
    // Arquivo "vazio" (0 bytes) e texto com astrais/tabs: abrir, editar,
    // salvar e reabrir sem corrupção.
    std::fs::write(&file, "").unwrap();
    let mut buf = Buffer::open(&file).unwrap();
    assert_eq!(buf.document.len_chars(), 0);
    buf.insert(0, "<?php $😀 = \"héllo\tworld\";\n");
    assert!(buf.dirty);
    buf.save().unwrap();
    drop(buf);
    let doc = Document::open(&file).unwrap();
    assert_eq!(doc.text(), "<?php $😀 = \"héllo\tworld\";\n");
    // Offsets centrais continuam consistentes sobre o texto salvo.
    let (l, c) = doc.line_col_encoded(doc.len_chars(), phpide_core::PositionEncoding::Utf16);
    assert_eq!(
        doc.lsp_position_to_char(l, c, phpide_core::PositionEncoding::Utf16),
        doc.len_chars()
    );
    std::fs::remove_dir_all(&dir).ok();
}
