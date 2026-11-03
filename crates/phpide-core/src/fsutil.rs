//! Escrita atômica de arquivos — base do `T-NFR-007-01` (NFR-007).
//!
//! `write_atomic` grava num temporário do mesmo diretório, faz fsync e
//! renomeia por cima do destino: leitores veem o arquivo antigo ou o novo,
//! nunca conteúdo parcial. Em qualquer falha o arquivo original permanece
//! intacto e o temporário é removido.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Nome do temporário usado na escrita atômica (visível apenas em crash).
fn tmp_name(path: &Path) -> io::Result<std::path::PathBuf> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "caminho sem nome de arquivo")
    })?;
    Ok(dir.join(format!(".{}.phpide-tmp", name.to_string_lossy())))
}

/// Grava `contents` em `path` de forma atômica (ou falha sem tocar em `path`).
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let tmp = tmp_name(path)?;
    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        sync_dir(path)?;
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        // Nunca deixar temporário para trás nem arquivo parcial no destino.
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// fsync do diretório para durabilizar o rename (best effort; alguns FS
/// não suportam — o rename atômico em si não depende disso).
fn sync_dir(path: &Path) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    match fs::File::open(dir) {
        Ok(f) => f.sync_all(),
        Err(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("phpide-fsutil-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn atomic_write_roundtrip() {
        let dir = scratch("roundtrip");
        let file = dir.join("a.php");
        write_atomic(&file, b"<?php // v1").unwrap();
        write_atomic(&file, b"<?php // v2").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "<?php // v2");
        assert!(!file.with_file_name(".a.php.phpide-tmp").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn failed_write_preserves_original_and_cleans_tmp() {
        let dir = scratch("preserve");
        let file = dir.join("b.php");
        write_atomic(&file, b"original").unwrap();
        // Destino vira um diretório: o rename (e a gravação do destino) falha.
        fs::remove_file(&file).unwrap();
        fs::create_dir(&file).unwrap();
        let tmp = dir.join(".b.php.phpide-tmp");
        fs::write(&tmp, b"conteudo parcial simulado").unwrap();
        let result = write_atomic(&file, b"novo conteudo completo");
        assert!(result.is_err(), "salvar sobre diretório deve falhar");
        // Sem temporário órfão e o diretório destino intacto.
        assert!(!tmp.exists(), "tmp órfão removido");
        assert!(file.is_dir(), "destino original preservado");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_parent_fails_without_creating_anything() {
        let dir = scratch("missing");
        let file = dir.join("sem/pai/c.php");
        assert!(write_atomic(&file, b"x").is_err());
        assert!(!dir.join("sem").exists(), "nada criado em árvore ausente");
        fs::remove_dir_all(&dir).ok();
    }
}
