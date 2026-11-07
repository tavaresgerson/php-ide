//! Modelo e estados de breakpoints — `T-RUN-DBG-003-01` (RUN-DBG-003).
//!
//! Estados refletem o backend (Xdebug): `Verified` (confirmado pelo
//! breakpoint_set), `Invalid` (recusado, com mensagem do backend),
//! `Disabled` (existe na IDE, não enviado). `Pending` é o estado antes da
//! confirmação — nunca apresentamos breakpoint novo como verificado sem
//! resposta do backend. A sincronização (`sync`) faz o diff entre o store e
//! o backend: cria/altera/remove comandos DBGp e atualiza estados pelas
//! respostas. Mute global (RUN-DBG-003 P1) suspende o envio sem apagar os
//! pontos.

use std::path::{Path, PathBuf};

use crate::mapping::PathMappings;
use crate::proto::{error_message, DbgpSession};

/// Tipo de breakpoint (RUN-DBG-003; Line é o P0, demais P1 preparados no modelo).
#[derive(Debug, Clone, PartialEq)]
pub enum BreakpointKind {
    Line,
    /// Condição PHP avaliada no backend.
    Conditional(String),
    /// Nome de exceção (FQN).
    Exception(String),
    /// Nome de método (FQN).
    Method(String),
    /// Logpoint: mensagem avaliada quando atingido.
    Logpoint(String),
}

/// Estado de verificação vindo do backend (ou claramente local).
#[derive(Debug, Clone, PartialEq)]
pub enum BreakpointStatus {
    /// Ainda não sincronizado com o backend.
    Pending,
    /// Confirmado pelo backend.
    Verified,
    /// Recusado pelo backend (mensagem explicativa).
    Invalid(String),
    /// Desabilitado localmente: não é enviado.
    Disabled,
}

impl BreakpointStatus {
    pub const fn as_str(&self) -> &'static str {
        match self {
            BreakpointStatus::Pending => "pending",
            BreakpointStatus::Verified => "verified",
            BreakpointStatus::Invalid(_) => "invalid",
            BreakpointStatus::Disabled => "disabled",
        }
    }
}

/// Breakpoint registrado na IDE.
#[derive(Debug, Clone)]
pub struct Breakpoint {
    /// ID local estável (UI e gutter).
    pub id: u64,
    /// ID atribuído pelo backend no breakpoint_set.
    pub backend_id: Option<String>,
    pub kind: BreakpointKind,
    /// Arquivo alvo (line/conditional/logpoint).
    pub file: Option<PathBuf>,
    /// Linha alvo (1-based).
    pub line: u32,
    pub status: BreakpointStatus,
    /// Número de vezes que o breakpoint interrompeu a sessão corrente.
    pub hit_count: u32,
}

/// Store de breakpoints com sincronização ao backend.
#[derive(Debug, Default)]
pub struct BreakpointStore {
    next_id: u64,
    pub breakpoints: Vec<Breakpoint>,
    /// Mute all: nada é enviado ao backend (os pontos ficam na IDE).
    pub muted: bool,
}

/// Resultado da sincronização de um breakpoint.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncEvent {
    Set {
        local_id: u64,
        backend_id: String,
    },
    Invalid {
        local_id: u64,
        message: String,
    },
    Removed {
        local_id: u64,
    },
    /// Sessão ausente/encerrada — estado permanece Pending.
    Deferred {
        local_id: u64,
    },
}

impl BreakpointStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Alterna breakpoint de linha em (arquivo, linha). Devolve `true` se
    /// ficou presente (toggle).
    pub fn toggle_line(&mut self, file: PathBuf, line: u32) -> bool {
        if let Some(pos) = self
            .breakpoints
            .iter()
            .position(|b| b.file.as_deref() == Some(file.as_path()) && b.line == line)
        {
            self.breakpoints.remove(pos);
            false
        } else {
            self.push(BreakpointKind::Line, Some(file), line);
            true
        }
    }

    /// Adiciona breakpoint de linha (sem toggle).
    pub fn add_line(&mut self, file: PathBuf, line: u32) -> u64 {
        self.push(BreakpointKind::Line, Some(file), line)
    }

    /// Define condição de um breakpoint de linha existente (ou cria).
    pub fn set_condition(&mut self, file: &Path, line: u32, condition: String) {
        let found = self
            .breakpoints
            .iter_mut()
            .find(|b| b.file.as_deref() == Some(file) && b.line == line);
        match found {
            Some(bp) => {
                bp.kind = BreakpointKind::Conditional(condition);
                bp.status = BreakpointStatus::Pending;
            }
            None => {
                self.push(
                    BreakpointKind::Conditional(condition),
                    Some(file.to_path_buf()),
                    line,
                );
            }
        }
    }

    fn push(&mut self, kind: BreakpointKind, file: Option<PathBuf>, line: u32) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.breakpoints.push(Breakpoint {
            id,
            backend_id: None,
            kind,
            file,
            line,
            status: BreakpointStatus::Pending,
            hit_count: 0,
        });
        id
    }

    pub fn remove(&mut self, local_id: u64) -> Option<Breakpoint> {
        let pos = self.breakpoints.iter().position(|b| b.id == local_id)?;
        Some(self.breakpoints.remove(pos))
    }

    pub fn get(&self, local_id: u64) -> Option<&Breakpoint> {
        self.breakpoints.iter().find(|b| b.id == local_id)
    }

    pub fn get_mut(&mut self, local_id: u64) -> Option<&mut Breakpoint> {
        self.breakpoints.iter_mut().find(|b| b.id == local_id)
    }

    /// Breakpoint de linha em (file, line), se existir.
    pub fn line_at(&self, file: &Path, line: u32) -> Option<&Breakpoint> {
        self.breakpoints
            .iter()
            .find(|b| b.file.as_deref() == Some(file) && b.line == line)
    }

    /// Encontra breakpoint por localização (para contar hits em paradas).
    pub fn hit_at(&mut self, file: &Path, line: u32) -> Option<u64> {
        let bp = self
            .breakpoints
            .iter_mut()
            .find(|b| b.file.as_deref() == Some(file) && b.line == line)?;
        bp.hit_count += 1;
        Some(bp.id)
    }

    /// Sincroniza o store com o backend: envia pontos pendentes (ou mutados)
    /// e remove do backend os que sumiram da IDE. Estados vêm das respostas.
    pub fn sync(
        &mut self,
        session: &mut Option<&mut DbgpSession>,
        mappings: &PathMappings,
    ) -> Vec<SyncEvent> {
        let mut events = Vec::new();
        if self.muted {
            return events;
        }
        // Coleta de operações primeiro (evita borrow conflict com session).
        // kind é clonado para montar os argumentos DBGp fora do borrow.
        let mut ops: Vec<(u64, BreakpointKind, PathBuf, u32)> = Vec::new();
        for bp in &self.breakpoints {
            if matches!(bp.status, BreakpointStatus::Disabled) {
                continue;
            }
            if bp.backend_id.is_some() && !matches!(bp.status, BreakpointStatus::Pending) {
                continue; // já sincronizado
            }
            // Exception/Method não têm arquivo; usam defaults (ignorados nos
            // argumentos do tipo).
            let file = bp.file.clone().unwrap_or_default();
            ops.push((bp.id, bp.kind.clone(), file, bp.line));
        }

        for (local_id, kind, file, line) in ops {
            let Some(session) = session.as_deref_mut() else {
                events.push(SyncEvent::Deferred { local_id });
                continue;
            };
            let uri = mappings.to_remote(&file);
            let args = kind.dbgp_args(&uri, line);
            let refs: Vec<(&str, &str)> =
                args.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            match session.send_xml("breakpoint_set", &refs) {
                Ok(xml) => {
                    let success = crate::proto::parse_response(&xml).is_some_and(|r| r.success);
                    if success {
                        let backend_id = crate::proto::attr(&xml, "id").unwrap_or("").to_string();
                        if let Some(bp) = self.get_mut(local_id) {
                            bp.backend_id = Some(backend_id.clone());
                            bp.status = BreakpointStatus::Verified;
                        }
                        events.push(SyncEvent::Set {
                            local_id,
                            backend_id,
                        });
                    } else {
                        let message = error_message(&xml)
                            .unwrap_or_else(|| "breakpoint recusado pelo backend".to_string());
                        if let Some(bp) = self.get_mut(local_id) {
                            bp.status = BreakpointStatus::Invalid(message.clone());
                        }
                        events.push(SyncEvent::Invalid { local_id, message });
                    }
                }
                Err(_) => events.push(SyncEvent::Deferred { local_id }),
            }
        }
        events
    }

    /// Remove um breakpoint também do backend (quando a sessão está ativa).
    pub fn remove_synced(
        &mut self,
        local_id: u64,
        session: &mut Option<&mut DbgpSession>,
    ) -> Option<SyncEvent> {
        let bp = self.remove(local_id)?;
        let Some(backend_id) = bp.backend_id else {
            return Some(SyncEvent::Removed { local_id });
        };
        let Some(session) = session.as_deref_mut() else {
            return Some(SyncEvent::Removed { local_id });
        };
        let id = backend_id.clone();
        let _ = session.send("breakpoint_remove", &[("d", &id)]);
        Some(SyncEvent::Removed { local_id })
    }
}

impl BreakpointKind {
    /// Argumentos DBGp do breakpoint_set por tipo (spec DBGp/Xdebug 3).
    /// Line/Conditional/Logpoint carregam f= (URI mapeada) e n= (linha);
    /// Exception usa -x (nome); Method usa -m (função).
    fn dbgp_args(&self, uri: &str, line: u32) -> Vec<(String, String)> {
        match self {
            BreakpointKind::Line => vec![
                ("t".into(), "line".into()),
                ("f".into(), uri.to_string()),
                ("n".into(), line.to_string()),
            ],
            BreakpointKind::Conditional(expr) | BreakpointKind::Logpoint(expr) => vec![
                ("t".into(), "line".into()),
                ("f".into(), uri.to_string()),
                ("n".into(), line.to_string()),
                ("o".into(), expr.clone()),
            ],
            BreakpointKind::Exception(name) => {
                vec![("t".into(), "exception".into()), ("x".into(), name.clone())]
            }
            BreakpointKind::Method(name) => {
                vec![("t".into(), "call".into()), ("m".into(), name.clone())]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_line_adds_and_removes() {
        let mut store = BreakpointStore::new();
        let file = PathBuf::from("/p/a.php");
        assert!(store.toggle_line(file.clone(), 10));
        assert_eq!(store.breakpoints.len(), 1);
        assert!(!store.toggle_line(file.clone(), 10));
        assert!(store.breakpoints.is_empty());
    }

    #[test]
    fn new_breakpoint_is_pending_not_verified() {
        let mut store = BreakpointStore::new();
        let id = store.add_line(PathBuf::from("/p/a.php"), 3);
        assert_eq!(store.get(id).unwrap().status.as_str(), "pending");
    }

    #[test]
    fn set_condition_marks_pending_again() {
        let mut store = BreakpointStore::new();
        let file = PathBuf::from("/p/a.php");
        let id = store.add_line(file.clone(), 3);
        store.get_mut(id).unwrap().status = BreakpointStatus::Verified;
        store.set_condition(&file, 3, "$x > 1".into());
        let bp = store.get(id).unwrap();
        assert!(matches!(&bp.kind, BreakpointKind::Conditional(c) if c == "$x > 1"));
        assert_eq!(bp.status.as_str(), "pending");
    }

    #[test]
    fn hit_at_increments_and_returns_id() {
        let mut store = BreakpointStore::new();
        let file = PathBuf::from("/p/a.php");
        let id = store.add_line(file.clone(), 3);
        assert_eq!(store.hit_at(&file, 3), Some(id));
        assert_eq!(store.hit_at(&file, 3), Some(id));
        assert_eq!(store.get(id).unwrap().hit_count, 2);
        assert_eq!(store.hit_at(&file, 99), None);
    }

    #[test]
    fn sync_without_session_defers() {
        let mut store = BreakpointStore::new();
        let id = store.add_line(PathBuf::from("/p/a.php"), 3);
        let mappings = PathMappings::new(PathBuf::from("/p"));
        let mut session = None;
        let events = store.sync(&mut session, &mappings);
        assert_eq!(events, vec![SyncEvent::Deferred { local_id: id }]);
        assert_eq!(store.get(id).unwrap().status.as_str(), "pending");
    }

    #[test]
    fn remove_synced_returns_event() {
        let mut store = BreakpointStore::new();
        let id = store.add_line(PathBuf::from("/p/a.php"), 3);
        let mut session = None;
        let event = store.remove_synced(id, &mut session).unwrap();
        assert_eq!(event, SyncEvent::Removed { local_id: id });
        assert!(store.get(id).is_none());
    }
}
