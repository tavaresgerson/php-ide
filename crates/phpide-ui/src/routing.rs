use std::path::{Path, PathBuf};
use std::sync::mpsc;

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LspOrigin {
    Php,
    TypeScript,
    Html,
    Css,
}

impl LspOrigin {
    pub fn from_server_name(name: &str) -> Option<Self> {
        match name {
            "TypeScript" => Some(Self::TypeScript),
            "HTML" => Some(Self::Html),
            "CSS" => Some(Self::Css),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestOperation {
    Definition,
    Implementation,
    TypeDefinition,
    /// Go to Super Method.
    Declaration,
    Hints,
    Usages,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RequestKey {
    pub origin: LspOrigin,
    pub operation: RequestOperation,
    pub path: PathBuf,
    pub revision: u64,
}

impl RequestKey {
    pub fn is_current(&self, path: &Path, revision: u64) -> bool {
        self.path == path && self.revision == revision
    }
}

pub struct PendingRequest {
    pub key: RequestKey,
    receiver: mpsc::Receiver<Value>,
}

#[derive(Default)]
pub struct LspRouting {
    requests: Vec<PendingRequest>,
}

impl LspRouting {
    pub fn insert(&mut self, key: RequestKey, receiver: mpsc::Receiver<Value>) {
        self.requests.retain(|request| request.key != key);
        self.requests.push(PendingRequest { key, receiver });
    }

    pub fn contains(&self, key: &RequestKey) -> bool {
        self.requests.iter().any(|request| &request.key == key)
    }

    pub fn clear(&mut self) {
        self.requests.clear();
    }

    pub fn clear_origin(&mut self, origin: LspOrigin) {
        self.requests.retain(|request| request.key.origin != origin);
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    // Only the frontend gates (experimental feature) use this.
    #[cfg(all(test, feature = "experimental"))]
    pub fn len(&self) -> usize {
        self.requests.len()
    }

    // Only the frontend gates (experimental feature) use this.
    #[cfg(all(test, feature = "experimental"))]
    pub fn has_operation(&self, operation: RequestOperation) -> bool {
        self.requests
            .iter()
            .any(|request| request.key.operation == operation)
    }

    pub fn poll(&mut self) -> Vec<(RequestKey, Value)> {
        let mut completed = Vec::new();
        self.requests
            .retain_mut(|request| match request.receiver.try_recv() {
                Ok(value) => {
                    completed.push((request.key.clone(), value));
                    false
                }
                Err(mpsc::TryRecvError::Empty) => true,
                Err(mpsc::TryRecvError::Disconnected) => false,
            });
        completed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(origin: LspOrigin, operation: RequestOperation) -> RequestKey {
        RequestKey {
            origin,
            operation,
            path: PathBuf::from("/project/main.ts"),
            revision: 7,
        }
    }

    #[test]
    fn simultaneous_servers_and_operations_do_not_overwrite_each_other() {
        let mut pending = LspRouting::default();
        let (php_tx, php_rx) = mpsc::channel();
        let (ts_tx, ts_rx) = mpsc::channel();
        let (hint_tx, hint_rx) = mpsc::channel();
        pending.insert(key(LspOrigin::Php, RequestOperation::Definition), php_rx);
        pending.insert(
            key(LspOrigin::TypeScript, RequestOperation::Definition),
            ts_rx,
        );
        pending.insert(key(LspOrigin::TypeScript, RequestOperation::Hints), hint_rx);
        ts_tx.send(serde_json::json!({"server": "ts"})).unwrap();
        php_tx.send(serde_json::json!({"server": "php"})).unwrap();
        hint_tx.send(serde_json::json!([])).unwrap();
        let mut completed = pending.poll();
        completed.sort_by_key(|(key, _)| (key.origin as u8, key.operation as u8));
        assert_eq!(completed.len(), 3);
        assert!(pending.is_empty());
        assert!(completed
            .iter()
            .any(|(key, value)| key.origin == LspOrigin::TypeScript && value["server"] == "ts"));
    }

    #[test]
    fn stale_document_response_is_identifiable() {
        let key = key(LspOrigin::TypeScript, RequestOperation::Definition);
        assert!(key.is_current(Path::new("/project/main.ts"), 7));
        assert!(!key.is_current(Path::new("/project/main.ts"), 8));
        assert!(!key.is_current(Path::new("/project/other.ts"), 7));
    }
}
