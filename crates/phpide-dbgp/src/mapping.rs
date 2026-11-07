//! Path mapping e associação de conexão ao projeto — `T-RUN-DBG-001-02`.
//!
//! O Xdebug conecta de fora (container, VM, remote host) com caminhos que
//! não são os locais. [`PathMappings`] traduz local↔remoto por prefixo
//! mais longo primeiro. A associação ao projeto usa o `fileuri` do `<init>`:
//! quando o script inicial resolve para dentro da raiz do projeto, a sessão
//! é do projeto; caso contrário é estrangeira e a UI deve pedir confirmação
//! (ou um mapping) antes de tratar breakpoints/locais — RUN-DBG-001 aceite.

use std::path::{Path, PathBuf};

/// Par de prefixos (local, remoto) ordenados por especificidade.
#[derive(Debug, Clone, Default)]
pub struct PathMappings {
    project_root: PathBuf,
    pairs: Vec<(PathBuf, String)>,
}

/// Resultado da associação de uma conexão DBGp ao projeto.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionMatch {
    /// Script inicial resolve para dentro do projeto (caminho local).
    Owned(PathBuf),
    /// Caminho mapeável, mas fora do projeto — exige confirmação/mapping.
    Foreign(PathBuf),
    /// fileuri ilegível — tratar como estrangeira sem caminho.
    Unknown,
}

impl PathMappings {
    pub fn new(project_root: PathBuf) -> Self {
        PathMappings {
            project_root,
            pairs: Vec::new(),
        }
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Registra mapping local→remoto (prefixo de diretório).
    pub fn add(&mut self, local: PathBuf, remote: String) {
        self.pairs.push((local, remote));
        // Mais específico (prefixo mais longo) primeiro.
        self.pairs
            .sort_by_key(|pair| std::cmp::Reverse(pair.0.components().count()));
    }

    pub fn pairs(&self) -> &[(PathBuf, String)] {
        &self.pairs
    }

    /// Caminho local → URI remoto para breakpoint_set.
    ///
    /// Sem mapping aplicável, devolve o fileuri do próprio caminho local
    /// (debug local/zero-config — o comum no fluxo CLI).
    pub fn to_remote(&self, local: &Path) -> String {
        for (lprefix, rprefix) in &self.pairs {
            if let Ok(rest) = local.strip_prefix(lprefix) {
                let rest = rest.to_string_lossy();
                let joined = if rest.is_empty() {
                    rprefix.clone()
                } else {
                    format!("{}/{}", rprefix.trim_end_matches('/'), rest)
                };
                return crate::proto::fileuri(Path::new(&joined));
            }
        }
        crate::proto::fileuri(local)
    }

    /// URI/caminho remoto → caminho local (stack_get, xdebug:message).
    pub fn to_local(&self, uri_or_path: &str) -> Option<PathBuf> {
        let remote_path = uri_or_path.strip_prefix("file://").unwrap_or(uri_or_path);
        let remote_path = percent_decode(remote_path);
        for (lprefix, rprefix) in &self.pairs {
            let rprefix = rprefix.trim_end_matches('/');
            if remote_path == rprefix {
                return Some(lprefix.clone());
            }
            if let Some(rest) = remote_path.strip_prefix(&format!("{rprefix}/")) {
                return Some(lprefix.join(rest));
            }
        }
        // Sem mapping: aceita caminho absoluto direto (debug local).
        let p = PathBuf::from(&remote_path);
        p.is_absolute().then_some(p)
    }

    /// Classifica o fileuri do `<init>` em relação ao projeto.
    pub fn classify(&self, fileuri: &str) -> ConnectionMatch {
        let Some(local) = self.to_local(fileuri) else {
            return ConnectionMatch::Unknown;
        };
        if local.starts_with(&self.project_root) {
            ConnectionMatch::Owned(local)
        } else {
            ConnectionMatch::Foreign(local)
        }
    }
}

fn percent_decode(s: &str) -> String {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// uri_to_path re-exportado para quem precisa só da conversão.
pub use crate::proto::uri_to_path as any_uri_to_path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_debug_roundtrip_without_mappings() {
        let mappings = PathMappings::new(PathBuf::from("/proj"));
        let local = PathBuf::from("/proj/src/a.php");
        let uri = mappings.to_remote(&local);
        assert_eq!(mappings.to_local(&uri), Some(local.clone()));
        assert_eq!(mappings.classify(&uri), ConnectionMatch::Owned(local));
    }

    #[test]
    fn longest_prefix_wins() {
        let mut mappings = PathMappings::new(PathBuf::from("/home/me/proj"));
        mappings.add(PathBuf::from("/home/me/proj"), "/var/www".to_string());
        mappings.add(PathBuf::from("/home/me/proj/app"), "/srv/app".to_string());
        assert_eq!(
            mappings.to_remote(Path::new("/home/me/proj/app/x.php")),
            "file:///srv/app/x.php"
        );
        assert_eq!(
            mappings.to_remote(Path::new("/home/me/proj/other/y.php")),
            "file:///var/www/other/y.php"
        );
    }

    #[test]
    fn remote_to_local_applies_mapping() {
        let mut mappings = PathMappings::new(PathBuf::from("/home/me/proj"));
        mappings.add(PathBuf::from("/home/me/proj"), "/var/www".to_string());
        assert_eq!(
            mappings.to_local("file:///var/www/src/a.php"),
            Some(PathBuf::from("/home/me/proj/src/a.php"))
        );
        // Resolvido para dentro do projeto → Owned.
        assert_eq!(
            mappings.classify("file:///var/www/src/a.php"),
            ConnectionMatch::Owned(PathBuf::from("/home/me/proj/src/a.php"))
        );
        // Mapping com raiz irmã do projeto: resolve, mas fora da raiz →
        // Foreign (a UI pede confirmação antes de aplicar breakpoints).
        let mut estranha = PathMappings::new(PathBuf::from("/home/me/proj"));
        estranha.add(PathBuf::from("/home/me/shared"), "/srv/shared".to_string());
        assert_eq!(
            estranha.classify("file:///srv/shared/lib/b.php"),
            ConnectionMatch::Foreign(PathBuf::from("/home/me/shared/lib/b.php"))
        );
        // Caminho remoto fora de qualquer prefixo: resolve como absoluto, mas
        // fora do projeto → Foreign (a IDE não aplica breakpoints nele).
        assert_eq!(
            mappings.classify("file:///outro/lugar/b.php"),
            ConnectionMatch::Foreign(PathBuf::from("/outro/lugar/b.php"))
        );
        // URI ilegível (não-file, não absoluto) → Unknown.
        assert_eq!(mappings.classify("eval:///?"), ConnectionMatch::Unknown);
    }

    #[test]
    fn percent_decoding_in_remote() {
        let mut mappings = PathMappings::new(PathBuf::from("/proj"));
        mappings.add(PathBuf::from("/proj"), "/var/www".to_string());
        assert_eq!(
            mappings.to_local("file:///var/www/ma%20os/a.php"),
            Some(PathBuf::from("/proj/ma os/a.php"))
        );
    }
}
