//! Busca em arquivos (Estágio 2; NFR-001 em T-NFR-001-02/03).
//!
//! Roda num worker do `JobSystem` (phpide-core) e envia resultados por
//! canal; a UI faz poll por frame, sem bloquear a thread de desenho. Cada
//! busca é um job com ID: nova busca cancela a anterior e hits de job
//! obsoleto são descartados (resultado atrasado nunca sobrescreve o atual).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
#[cfg(test)]
use std::time::{Duration, Instant};

use phpide_core::jobs::{JobHandle, JobId, JobSystem};

const MAX_HITS: usize = 500;
const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub path: PathBuf,
    /// 1-based para exibição.
    pub line: usize,
    pub preview: String,
}

enum SearchMsg {
    Hit(JobId, SearchHit),
    Done(JobId, usize, bool),
}

/// Arquivos de texto que entram na busca (código e config comuns em projetos PHP).
pub fn is_searchable(path: &Path) -> bool {
    const EXT: [&str; 14] = [
        "php", "phtml", "inc", "json", "xml", "yml", "yaml", "md", "txt", "js", "ts", "css",
        "html", "env",
    ];
    path.extension()
        .map(|e| EXT.contains(&e.to_string_lossy().to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub struct SearchState {
    pub query: String,
    pub case_sensitive: bool,
    pub results: Vec<SearchHit>,
    pub status: String,
    rx: Option<Receiver<SearchMsg>>,
    /// Job da busca vigente (cancelar = stop; resultado obsoleto descartado).
    job: Option<JobHandle<()>>,
    current: Option<JobId>,
}

impl SearchState {
    pub fn new() -> Self {
        SearchState {
            query: String::new(),
            case_sensitive: false,
            results: Vec::new(),
            status: String::new(),
            rx: None,
            job: None,
            current: None,
        }
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// ID do job vigente (correlação de logs/descarte de obsoletos).
    pub fn current_job(&self) -> Option<JobId> {
        self.current
    }

    /// Cancela a busca em andamento (ex.: troca de projeto, nova busca).
    pub fn cancel(&mut self) {
        if let Some(job) = &self.job {
            job.cancel();
        }
        self.job = None;
        self.rx = None;
        self.current = None;
    }

    /// Dispara busca pelos arquivos fornecidos (já filtrados por
    /// `is_searchable`) num worker do `JobSystem`.
    pub fn start(&mut self, files: Vec<PathBuf>, system: &JobSystem) {
        if self.query.is_empty() {
            return;
        }
        // Nova busca torna obsoleta a anterior: cancela e limpa.
        self.cancel();
        self.results.clear();
        self.status = "buscando...".to_string();
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let needle = if self.case_sensitive {
            self.query.clone()
        } else {
            self.query.to_lowercase()
        };
        let case_sensitive = self.case_sensitive;
        let handle = system.spawn(move |ctx| {
            let mut searched = 0usize;
            let mut sent = 0usize;
            let mut truncated = false;
            'files: for path in files {
                if ctx.is_cancelled() {
                    break 'files;
                }
                if sent >= MAX_HITS {
                    truncated = true;
                    break 'files;
                }
                let Ok(bytes) = fs::read(&path) else { continue };
                if bytes.len() > MAX_FILE_BYTES {
                    continue;
                }
                let Ok(text) = String::from_utf8(bytes) else {
                    continue;
                };
                searched += 1;
                for (i, raw) in text.lines().enumerate() {
                    let hay = if case_sensitive {
                        raw.to_string()
                    } else {
                        raw.to_lowercase()
                    };
                    if hay.contains(&needle) {
                        sent += 1;
                        let hit = SearchHit {
                            path: path.clone(),
                            line: i + 1,
                            preview: raw.trim().chars().take(140).collect(),
                        };
                        if tx.send(SearchMsg::Hit(ctx.id(), hit)).is_err() || sent >= MAX_HITS {
                            truncated = true;
                            break 'files;
                        }
                    }
                }
            }
            let _ = tx.send(SearchMsg::Done(ctx.id(), searched, truncated));
            Ok::<(), phpide_core::IdeError>(())
        });
        self.current = Some(handle.id);
        self.job = Some(handle);
    }

    /// Consome resultados prontos (chamar a cada frame). Hits e
    /// finalizações de jobs obsoletos são descartados (NFR-001).
    pub fn poll(&mut self) {
        let mut done = None;
        let current = self.current;
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    SearchMsg::Hit(id, h) if Some(id) == current => self.results.push(h),
                    SearchMsg::Hit(_, _) => {} // job obsoleto: descartar
                    SearchMsg::Done(id, searched, truncated) if Some(id) == current => {
                        done = Some((searched, truncated))
                    }
                    SearchMsg::Done(_, _, _) => {} // job obsoleto: descartar
                }
            }
        }
        if let Some(job) = &mut self.job {
            job.poll();
        }
        if let Some((searched, truncated)) = done {
            self.rx = None;
            self.job = None;
            self.current = None;
            self.status = format!(
                "{} arquivos, {} ocorrências{}",
                searched,
                self.results.len(),
                if truncated { " (truncado)" } else { "" }
            );
        }
    }

    /// Aguarda a busca terminar (testes).
    #[cfg(test)]
    pub fn wait(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while self.busy() && Instant::now() < deadline {
            self.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        self.poll();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_finds_hits_via_job_system() {
        let dir = std::env::temp_dir().join(format!("phpide-uisearch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.php"), "<?php $alvo = 1;\n// nada\n").unwrap();
        fs::write(dir.join("b.php"), "<?php // outro\n$alvo = 2;\n").unwrap();
        let sys = JobSystem::new();
        let mut search = SearchState::new();
        search.query = "alvo".to_string();
        search.start(vec![dir.join("a.php"), dir.join("b.php")], &sys);
        search.wait(Duration::from_secs(5));
        assert!(!search.busy());
        assert_eq!(search.results.len(), 2, "status: {}", search.status);
        assert!(search.current_job().is_none(), "job encerrado e limpo");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn restarting_search_cancels_previous_and_discards_stale() {
        let dir = std::env::temp_dir().join(format!("phpide-uisearch2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for i in 0..50 {
            fs::write(dir.join(format!("f{i:02}.php")), "<?php $velho = 1;\n").unwrap();
        }
        let sys = JobSystem::new();
        let mut search = SearchState::new();
        search.query = "velho".to_string();
        let files: Vec<PathBuf> = (0..50).map(|i| dir.join(format!("f{i:02}.php"))).collect();
        search.start(files.clone(), &sys);
        let first = search.current_job().expect("job da primeira busca");
        // Segunda busca imediatamente: nova requisição vigente, id distinto.
        search.start(files, &sys);
        let second = search.current_job().expect("job da segunda busca");
        assert_ne!(first, second, "restart gera job novo (anterior obsoleto)");
        search.wait(Duration::from_secs(5));
        assert!(!search.busy());
        assert!(search.current_job().is_none(), "job encerrado e limpo");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cancel_clears_running_search() {
        let dir = std::env::temp_dir().join(format!("phpide-uisearch3-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for i in 0..200 {
            fs::write(dir.join(format!("g{i:03}.php")), "<?php $x = 1;\n").unwrap();
        }
        let sys = JobSystem::new();
        let mut search = SearchState::new();
        search.query = "x".to_string();
        let files: Vec<PathBuf> = (0..200).map(|i| dir.join(format!("g{i:03}.php"))).collect();
        search.start(files, &sys);
        search.cancel();
        assert!(!search.busy(), "cancelamento limpa o estado");
        assert!(search.current_job().is_none());
        search.wait(Duration::from_secs(1));
        fs::remove_dir_all(&dir).ok();
    }
}
