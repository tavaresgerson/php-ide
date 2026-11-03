//! Infraestrutura de jobs — `T-NFR-001-02`, `T-NFR-001-03`, `T-NFR-001-04` (NFR-001).
//!
//! - Todo trabalho longo (filesystem extenso, rede, processos, LSP, Git, SQL)
//!   roda numa thread worker e devolve o resultado por canal; a UI faz poll
//!   por frame e nunca bloqueia (NFR-001).
//! - Cada job tem **ID estável** e **estado observável**: ociosa/em execução/
//!   concluída/cancelada/com erro.
//! - Cancelamento cooperativo por flag; [`JobRegistry`] descarta resultados
//!   obsoletos por chave lógica (busca, completion, outline).
//! - [`JobSystem::shutdown`] cancela e encerra workers ao trocar de projeto
//!   ou fechar a IDE (NFR-001).

use crate::error::IdeError;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Identificador estável de um job (correlação de logs, NFR-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub u64);

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Estado observável de um job (NFR-001: toda operação longa expõe estado).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    /// Não iniciada (reservado para filas futuras).
    Idle,
    /// Em execução.
    Running,
    /// Concluída com resultado.
    Completed,
    /// Cancelada pelo usuário, troca de projeto ou shutdown.
    Cancelled,
    /// Terminou com erro (ver `JobOutcome::Err`).
    Failed,
}

impl JobState {
    pub const fn is_finished(self) -> bool {
        matches!(
            self,
            JobState::Completed | JobState::Cancelled | JobState::Failed
        )
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            JobState::Idle => "idle",
            JobState::Running => "running",
            JobState::Completed => "completed",
            JobState::Cancelled => "cancelled",
            JobState::Failed => "failed",
        }
    }
}

/// Resultado final de um job.
#[derive(Debug)]
pub enum JobOutcome<T> {
    Ok(T),
    Err(IdeError),
    Cancelled,
}

/// Contexto entregue ao worker: identidade e cancelamento cooperativo.
/// O trabalho deve consultar [`JobContext::is_cancelled`] entre unidades de
/// progresso e devolver o que já tem.
pub struct JobContext {
    id: JobId,
    cancel: Arc<AtomicBool>,
}

impl JobContext {
    pub fn id(&self) -> JobId {
        self.id
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

type JoinSlot = Arc<Mutex<Option<JoinHandle<()>>>>;

/// Cabo de um job disparado: cancela, observa estado e drena o resultado.
pub struct JobHandle<T> {
    pub id: JobId,
    state: JobState,
    rx: Receiver<JobOutcome<T>>,
    cancel: Arc<AtomicBool>,
    join: JoinSlot,
}

impl<T> JobHandle<T> {
    /// Solicita cancelamento (cooperativo: o worker verifica a flag).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn state(&self) -> JobState {
        self.state
    }

    /// Drena um resultado disponível (chamar a cada frame). Só consome o
    /// receptor uma vez: o valor fica com quem chamou.
    pub fn poll(&mut self) -> Option<JobOutcome<T>> {
        if self.state.is_finished() {
            return None;
        }
        match self.rx.try_recv() {
            Ok(outcome) => {
                self.state = match &outcome {
                    JobOutcome::Ok(_) => JobState::Completed,
                    JobOutcome::Err(_) => JobState::Failed,
                    JobOutcome::Cancelled => JobState::Cancelled,
                };
                if let Some(h) = self.join.lock().unwrap().take() {
                    let _ = h.join();
                }
                Some(outcome)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.state = JobState::Failed;
                None
            }
        }
    }

    /// Aguarda até `timeout` por um resultado (para testes e shutdown fino).
    pub fn wait(&mut self, timeout: Duration) -> Option<JobOutcome<T>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(outcome) = self.poll() {
                return Some(outcome);
            }
            if Instant::now() >= deadline {
                self.cancel();
                return None;
            }
            thread::sleep(Duration::from_millis(2));
        }
    }
}

impl<T> Drop for JobHandle<T> {
    /// Soltar o cabo sem consumir o resultado cancela o job: nenhum
    /// worker órfão segue processando para um destinatário que partiu.
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

/// Resumo de um shutdown (NFR-001: troca de projeto / fechamento da IDE).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShutdownReport {
    /// Jobs que receberam pedido de cancelamento.
    pub cancelled: usize,
    /// Workers que encerraram dentro do prazo.
    pub joined: usize,
    /// Workers que não confirmaram no prazo (detached; a flag segue ativa).
    pub unresponsive: usize,
}

struct Entry {
    cancel: Weak<AtomicBool>,
    join: JoinSlot,
}

/// Fábrica e registro de jobs do processo (um por sessão da IDE).
#[derive(Default)]
pub struct JobSystem {
    next: AtomicU64,
    entries: Mutex<Vec<Entry>>,
}

impl JobSystem {
    pub fn new() -> Self {
        Self::default()
    }

    /// Dispara `work` numa thread worker. O resultado chega por
    /// [`JobHandle::poll`]; o worker deve honrar `ctx.is_cancelled()`.
    pub fn spawn<F, T>(&self, work: F) -> JobHandle<T>
    where
        F: FnOnce(&JobContext) -> Result<T, IdeError> + Send + 'static,
        T: Send + 'static,
    {
        let id = JobId(self.next.fetch_add(1, Ordering::SeqCst) + 1);
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut entries = self.entries.lock().unwrap();
            entries.retain(|e| e.cancel.upgrade().is_some());
            entries.push(Entry {
                cancel: Arc::downgrade(&cancel),
                join: Arc::new(Mutex::new(None)),
            });
        }
        let join_slot = {
            let entries = self.entries.lock().unwrap();
            entries.last().unwrap().join.clone()
        };
        let ctx_cancel = cancel.clone();
        let (tx, rx) = channel();
        let handle = thread::Builder::new()
            .name(format!("phpide-job-{}", id.0))
            .spawn(move || {
                let ctx = JobContext {
                    id,
                    cancel: ctx_cancel,
                };
                // Contrato: quem decide o que vale é o worker. Cancelado
                // ANTES da thread iniciar vira JobOutcome::Cancelled; depois
                // do início, o resultado do worker é entregue como está
                // (workers canceláveis devolvem valores que descrevem o
                // cancelamento — ex. Completed { cancelled: true }).
                let outcome = if ctx.is_cancelled() {
                    JobOutcome::Cancelled
                } else {
                    match work(&ctx) {
                        Ok(value) => JobOutcome::Ok(value),
                        Err(e) => JobOutcome::Err(e),
                    }
                };
                let _ = tx.send(outcome);
            })
            .expect("thread de job");
        *join_slot.lock().unwrap() = Some(handle);
        JobHandle {
            id,
            state: JobState::Running,
            rx,
            cancel,
            join: join_slot,
        }
    }

    /// Cancela todos os jobs vivos e aguarda até `join_timeout` cada um.
    /// Chamado ao trocar de projeto (isolar trabalho do contexto anterior)
    /// e ao fechar a IDE.
    pub fn shutdown(&self, join_timeout: Duration) -> ShutdownReport {
        let entries: Vec<Entry> = self.entries.lock().unwrap().drain(..).collect();
        let mut report = ShutdownReport {
            cancelled: entries.len(),
            ..Default::default()
        };
        for e in &entries {
            if let Some(cancel) = e.cancel.upgrade() {
                cancel.store(true, Ordering::SeqCst);
            }
        }
        let deadline = Instant::now() + join_timeout;
        for e in entries {
            let Some(join) = e.join.lock().unwrap().take() else {
                continue;
            };
            while !join.is_finished() {
                if Instant::now() >= deadline {
                    report.unresponsive += 1;
                    // JoinHandle solto aqui: a thread segue detached, mas a
                    // flag de cancelamento continua ativa e ela deve sair.
                    break;
                }
                thread::sleep(Duration::from_millis(2));
            }
            if join.is_finished() {
                let _ = join.join();
                report.joined += 1;
            }
        }
        report
    }
}

impl Drop for JobSystem {
    /// Fechamento da IDE: cancela o que estiver vivo sem bloquear o drop.
    fn drop(&mut self) {
        let entries: Vec<Entry> = self.entries.lock().unwrap().drain(..).collect();
        for e in entries {
            if let Some(cancel) = e.cancel.upgrade() {
                cancel.store(true, Ordering::SeqCst);
            }
        }
    }
}

/// Registro da requisição mais recente por chave lógica (ex.: "search",
/// ("completion", path)). Resultados que chegam de um job mais antigo que o
/// registrado são **descartados** (NFR-001: "resultados atrasados devem ser
/// associados a documento, revisão, sessão ou consulta; resultados
/// obsoletos devem ser descartados").
pub struct JobRegistry<K: Eq + Hash> {
    map: HashMap<K, JobId>,
}

impl<K: Eq + Hash> Default for JobRegistry<K> {
    fn default() -> Self {
        JobRegistry {
            map: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash> JobRegistry<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra `id` como a requisição vigente de `key`; devolve a anterior.
    pub fn register(&mut self, key: K, id: JobId) -> Option<JobId> {
        self.map.insert(key, id)
    }

    pub fn current(&self, key: &K) -> Option<JobId> {
        self.map.get(key).copied()
    }

    /// `true` se `id` ainda for a requisição vigente de `key`.
    pub fn is_current(&self, key: &K, id: JobId) -> bool {
        self.map.get(key) == Some(&id)
    }

    pub fn deregister(&mut self, key: &K) {
        self.map.remove(key);
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{Component, ErrorKind};
    use std::sync::atomic::{AtomicUsize, Ordering as AOrd};

    #[test]
    fn job_completes_with_value() {
        let sys = JobSystem::new();
        let mut h = sys.spawn(|_ctx| Ok::<_, IdeError>(42u8));
        match h.wait(Duration::from_secs(5)) {
            Some(JobOutcome::Ok(v)) => assert_eq!(v, 42),
            other => panic!("esperava Ok(42), veio {other:?}"),
        }
        assert_eq!(h.state(), JobState::Completed);
    }

    #[test]
    fn job_failure_carries_ide_error() {
        let sys = JobSystem::new();
        let mut h: JobHandle<()> = sys.spawn(|_ctx| {
            Err(IdeError::new(
                Component::Search,
                "scan",
                ErrorKind::Io,
                "falha de leitura",
                "tente novamente",
            ))
        });
        match h.wait(Duration::from_secs(5)) {
            Some(JobOutcome::Err(e)) => {
                assert_eq!(e.kind, ErrorKind::Io);
                assert_eq!(e.component, Component::Search);
            }
            other => panic!("esperava Err, veio {other:?}"),
        }
        assert_eq!(h.state(), JobState::Failed);
    }

    #[test]
    fn cancellation_is_observable_and_fast() {
        let sys = JobSystem::new();
        let started = Arc::new(AtomicUsize::new(0));
        let seen = started.clone();
        let mut h: JobHandle<()> = sys.spawn(move |ctx| {
            while !ctx.is_cancelled() {
                seen.fetch_add(1, AOrd::SeqCst);
                thread::sleep(Duration::from_millis(2));
            }
            // Worker cancelável: converte a flag no erro de domínio.
            Err(IdeError::cancelled(Component::Core, "loop"))
        });
        thread::sleep(Duration::from_millis(20));
        h.cancel();
        let t0 = Instant::now();
        match h.wait(Duration::from_secs(5)) {
            Some(JobOutcome::Err(e)) => {
                assert_eq!(e.kind, ErrorKind::Cancelled, "erro de cancelamento")
            }
            other => panic!("esperava Err(cancelled), veio {other:?}"),
        }
        assert!(started.load(AOrd::SeqCst) > 0, "o worker rodou");
        assert!(
            t0.elapsed() < Duration::from_secs(2),
            "cancelamento deve encerrar rápido"
        );
        assert_eq!(h.state(), JobState::Failed);
    }

    #[test]
    fn registry_discards_stale_results() {
        let mut reg = JobRegistry::new();
        let k = "search";
        reg.register(k, JobId(1));
        assert!(reg.is_current(&k, JobId(1)));
        // Nova busca dispara: resultado do job 1 agora é obsoleto.
        reg.register(k, JobId(2));
        assert!(!reg.is_current(&k, JobId(1)), "job antigo é descartado");
        assert!(reg.is_current(&k, JobId(2)));
        reg.deregister(&k);
        assert!(!reg.is_current(&k, JobId(2)));
    }

    #[test]
    fn stale_discard_end_to_end_out_of_order_completion() {
        // Dois jobs para a mesma chave lógica terminam fora de ordem;
        // só o mais recente pode ser aplicado.
        let sys = JobSystem::new();
        let mut reg = JobRegistry::new();
        let mut slow = sys.spawn(|ctx| {
            while !ctx.is_cancelled() {
                thread::sleep(Duration::from_millis(5));
            }
            Ok::<_, IdeError>("slow")
        });
        reg.register("outline", slow.id);
        let mut fast = sys.spawn(|_ctx| Ok::<_, IdeError>("fast"));
        reg.register("outline", fast.id);
        let applied = match fast.wait(Duration::from_secs(5)) {
            Some(JobOutcome::Ok(v)) if reg.is_current(&"outline", fast.id) => v,
            other => panic!("resultado do job novo não aplicável: {other:?}"),
        };
        assert_eq!(applied, "fast");
        // O job velho, quando terminar, deve ser descartado pelo registry.
        slow.cancel();
        assert!(
            slow.wait(Duration::from_secs(5)).is_some(),
            "job antigo termina após cancelamento"
        );
        assert!(!reg.is_current(&"outline", slow.id));
    }

    #[test]
    fn shutdown_cancels_and_joins_all() {
        let sys = JobSystem::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..3 {
            let c = counter.clone();
            // Handles ficam vivos: soltar um handle cancela o job (testado
            // separadamente), e aqui queremos os 3 no ar no shutdown.
            handles.push(sys.spawn(move |ctx| {
                while !ctx.is_cancelled() {
                    c.fetch_add(1, AOrd::SeqCst);
                    thread::sleep(Duration::from_millis(2));
                }
                Ok::<_, IdeError>(())
            }));
        }
        thread::sleep(Duration::from_millis(15));
        let report = sys.shutdown(Duration::from_secs(5));
        assert_eq!(report.cancelled, 3);
        assert_eq!(report.unresponsive, 0, "workers cooperativos encerram");
        assert_eq!(report.joined, 3);
        assert!(counter.load(AOrd::SeqCst) > 0);
        drop(handles);
    }

    #[test]
    fn dropping_handle_cancels_job() {
        let sys = JobSystem::new();
        let flag = Arc::new(AtomicUsize::new(0));
        let f2 = flag.clone();
        let h: JobHandle<()> = sys.spawn(move |ctx| {
            while !ctx.is_cancelled() {
                f2.fetch_add(1, AOrd::SeqCst);
                thread::sleep(Duration::from_millis(2));
            }
            Ok::<(), IdeError>(())
        });
        thread::sleep(Duration::from_millis(10));
        let id = h.id;
        drop(h);
        let deadline = Instant::now() + Duration::from_secs(2);
        while flag.load(AOrd::SeqCst) > 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
        let _ = id;
        // O worker pode fazer mais uma iteração, mas deve parar.
        let before = flag.load(AOrd::SeqCst);
        thread::sleep(Duration::from_millis(20));
        assert!(
            flag.load(AOrd::SeqCst) <= before + 2,
            "worker órfão deve parar após drop do handle"
        );
    }
}
