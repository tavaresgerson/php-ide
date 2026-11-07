//! Gerenciador de sessões — `T-RUN-001-02/03/05` (RUN-001).
//!
//! - **Process list**: sessões em ordem de criação, com estado real
//!   (nunca conta processo morto como vivo);
//! - **allow multiple instances**: sem a flag, `start` de uma configuração
//!   com sessão viva apenas foca a sessão existente;
//! - **Rerun**: stop da sessão + novo spawn com o mesmo spec;
//! - **Pause Output** e **Pin Tab** por sessão;
//! - **Redaction** (T-RUN-001-05): tudo que entra no log passa por
//!   `redact_text`; env nunca é logada em claro — apenas o comando
//!   (programa + args) e as chaves de ambiente.

use phpide_core::log::{Correlation, Level, LogSink, Logger};
use phpide_core::redact::redact_text;
use phpide_core::Component;
use phpide_exec::CommandSpec;

use crate::config::RunConfig;
#[cfg(test)]
use crate::session::SessionState;
use crate::session::{RunSession, SessionId};

/// Lista de processos/sessões de execução da IDE (RUN-001).
pub struct RunManager {
    sessions: Vec<RunSession>,
    next_id: SessionId,
    logger: Logger,
}

impl Default for RunManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RunManager {
    pub fn new() -> Self {
        RunManager::with_sink(Box::new(phpide_core::log::MemorySink::new()))
    }

    /// Manager com sink de log dedicado (a UI conecta o Logger da sessão).
    pub fn with_sink(sink: Box<dyn LogSink>) -> Self {
        RunManager {
            sessions: Vec::new(),
            next_id: 1,
            logger: Logger::new(sink),
        }
    }

    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    /// Sessões em ordem de criação (a UI pode reordenar por pin/estado).
    pub fn sessions(&self) -> &[RunSession] {
        &self.sessions
    }

    pub fn session(&self, id: SessionId) -> Option<&RunSession> {
        self.sessions.iter().find(|s| s.id == id)
    }

    pub fn session_mut(&mut self, id: SessionId) -> Option<&mut RunSession> {
        self.sessions.iter_mut().find(|s| s.id == id)
    }

    /// Sessões vivas da configuração (allow multiple instances).
    pub fn live_for(&self, config_id: &str) -> Vec<SessionId> {
        self.sessions
            .iter()
            .filter(|s| s.config.id == config_id && s.state.is_live())
            .map(|s| s.id)
            .collect()
    }

    /// Inicia uma sessão para a configuração.
    ///
    /// Sem `allow_multiple_instances` e com sessão viva da mesma configuração,
    /// retorna a sessão existente (foco — não duplica processo).
    pub fn start(
        &mut self,
        config: RunConfig,
        spec: CommandSpec,
        plan: Option<crate::phpunit::PhpUnitPlan>,
    ) -> SessionId {
        if !config.allow_multiple_instances {
            if let Some(existing) = self
                .sessions
                .iter()
                .find(|s| s.config.id == config.id && s.state.is_live() && !s.is_stopping())
            {
                let id = existing.id;
                self.log_info(id, "start", "configuração já em execução — focando sessão");
                return id;
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        let corr = Correlation::session(id);
        self.logger.log(
            Level::Info,
            Component::Exec,
            &corr,
            "run_start",
            &[(
                "msg",
                &redact_text(&format!("{} — {}", config.name, spec.program)),
            )],
        );
        let session = RunSession::start(id, config, spec, plan);
        if let Some(failure) = &session.failure {
            self.logger.log(
                Level::Error,
                Component::Exec,
                &corr,
                "run_start",
                &[("msg", &redact_text(failure))],
            );
        }
        self.sessions.push(session);
        id
    }

    /// Rerun (RUN-001): encerra a sessão e dispara outra com o mesmo spec
    /// e o mesmo plano de artefatos (ex.: Clover de cobertura).
    pub fn rerun(&mut self, id: SessionId) -> Option<SessionId> {
        let session = self.session_mut(id)?;
        let spec = session.spec().clone();
        let config = session.config.clone();
        let plan = session.plan.clone();
        if session.state.is_live() {
            session.stop();
            session.poll();
        }
        self.log_info(id, "rerun", "reexecutando configuração");
        Some(self.start(config, spec, plan))
    }

    pub fn stop(&mut self, id: SessionId) {
        if let Some(session) = self.session_mut(id) {
            if session.state.is_live() {
                session.stop();
                self.log_info(id, "run_stop", "stop solicitado");
            }
        }
    }

    /// Stop em todas as sessões vivas (troca de projeto/saída, NFR-001-04).
    pub fn stop_all(&mut self) {
        for session in &mut self.sessions {
            session.shutdown();
        }
    }

    /// Drena eventos de todas as sessões (chamar a cada frame).
    pub fn poll(&mut self) {
        let mut transitions = Vec::new();
        for session in &mut self.sessions {
            let before = session.state;
            session.poll();
            if session.state != before {
                transitions.push((session.id, before, session.state));
            }
        }
        for (id, before, after) in transitions {
            self.log_info(
                id,
                "run_state",
                &format!("{} → {}", before.as_str(), after.as_str()),
            );
        }
    }

    /// Remove sessões encerradas que não estão pinadas e excedem o teto de
    /// abas ( Closing Policy do painel Run — reciclagem das mais antigas).
    pub fn reclaim_finished(&mut self, keep: usize) {
        let finished: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|s| !s.state.is_live() && !s.pinned)
            .map(|s| s.id)
            .collect();
        let excess = finished.len().saturating_sub(keep);
        for id in finished.into_iter().take(excess) {
            self.sessions.retain(|s| s.id != id);
        }
    }

    fn log_info(&self, id: SessionId, operation: &str, message: &str) {
        self.logger.log(
            Level::Info,
            Component::Exec,
            &Correlation::session(id),
            operation,
            &[("msg", &redact_text(message))],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{RunConfig, RunKind};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn sh_config(name: &str, script: &str) -> (RunConfig, CommandSpec) {
        let cfg = RunConfig::new(
            name,
            name,
            RunKind::PhpScript {
                file: PathBuf::from("x.php"),
                args: vec![],
                interpreter_options: vec![],
                break_at_first_line: false,
            },
        );
        let spec = CommandSpec::new("sh", vec!["-c".into(), script.into()]);
        (cfg, spec)
    }

    fn launch(m: &mut RunManager, cfg: RunConfig, spec: CommandSpec) -> SessionId {
        m.start(cfg, spec, None)
    }

    fn wait_state(m: &mut RunManager, id: SessionId, want: SessionState, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            m.poll();
            if m.session(id).is_some_and(|s| s.state == want) {
                return;
            }
            assert!(Instant::now() < deadline, "timeout esperando {want:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn start_runs_and_finishes_with_exit_code() {
        let mut m = RunManager::new();
        let (cfg, spec) = sh_config("s1", "echo ola");
        let id = launch(&mut m, cfg, spec);
        wait_state(&mut m, id, SessionState::Finished, Duration::from_secs(10));
        let s = m.session(id).unwrap();
        assert_eq!(s.exit_code, Some(0));
        assert!(s.output().contains("ola"));
        assert!(s.command_line().contains("sh"));
    }

    #[test]
    fn non_multiple_instances_focuses_live_session() {
        let mut m = RunManager::new();
        let (cfg, spec) = sh_config("s1", "sleep 5");
        let first = launch(&mut m, cfg.clone(), spec.clone());
        let second = launch(&mut m, cfg, spec);
        assert_eq!(first, second, "sem allow_multiple: mesma sessão");
        assert_eq!(m.sessions().len(), 1);
        m.stop(second);
        wait_state(
            &mut m,
            second,
            SessionState::Stopped,
            Duration::from_secs(10),
        );
        // Depois de encerrada, um novo start cria outra sessão.
        let (cfg, spec) = sh_config("s1", "true");
        let third = launch(&mut m, cfg, spec);
        assert_ne!(third, second);
    }

    #[test]
    fn rerun_stops_and_restarts() {
        let mut m = RunManager::new();
        let (cfg, spec) = sh_config("s1", "sleep 5");
        let id = launch(&mut m, cfg, spec);
        std::thread::sleep(Duration::from_millis(50));
        let new_id = m.rerun(id).unwrap();
        assert_ne!(new_id, id);
        wait_state(&mut m, id, SessionState::Stopped, Duration::from_secs(10));
        wait_state(
            &mut m,
            new_id,
            SessionState::Finished,
            Duration::from_secs(10),
        );
    }

    #[test]
    fn pause_output_freezes_buffer_while_process_runs() {
        let mut m = RunManager::new();
        let (cfg, spec) = sh_config("s1", "echo antes; sleep 0.4; echo depois");
        let id = launch(&mut m, cfg, spec);
        // Espera a PRIMEIRA linha chegar — pausar antes disso descartaria
        // "antes" também (sem garantia de ordem).
        let deadline = Instant::now() + Duration::from_secs(5);
        while !m.session(id).is_some_and(|s| s.output().contains("antes")) {
            m.poll();
            assert!(
                Instant::now() < deadline,
                "timeout esperando primeira linha"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        m.session_mut(id).unwrap().set_output_paused(true);
        assert_eq!(m.session(id).unwrap().state, SessionState::PausedOutput);
        // Espera o processo produzir "depois" durante a pausa.
        std::thread::sleep(Duration::from_millis(600));
        m.poll();
        let s = m.session(id).unwrap();
        assert!(s.output().contains("antes"));
        assert!(!s.output().contains("depois"), "pausado: linha descartada");
        // Retoma: saída volta a ser anexada; o "depois" emitido na pausa
        // ficou descartado (sem buffer retroativo — documentado).
        m.session_mut(id).unwrap().set_output_paused(false);
        wait_state(&mut m, id, SessionState::Finished, Duration::from_secs(5));
    }

    #[test]
    fn stop_kills_process_group() {
        let mut m = RunManager::new();
        let (cfg, spec) = sh_config("s1", "sleep 30");
        let id = launch(&mut m, cfg, spec);
        wait_state(&mut m, id, SessionState::Running, Duration::from_secs(5));
        m.stop(id);
        wait_state(&mut m, id, SessionState::Stopped, Duration::from_secs(10));
        assert_eq!(m.session(id).unwrap().exit_code, None);
    }

    #[test]
    fn env_secrets_never_reach_log() {
        use phpide_core::log::LogSink;
        use std::sync::{Arc, Mutex};
        #[derive(Clone, Default)]
        struct Shared(Arc<Mutex<Vec<String>>>);
        impl LogSink for Shared {
            fn write_line(&self, line: &str) {
                self.0.lock().unwrap().push(line.to_string());
            }
        }
        let shared = Shared::default();
        let reader = shared.clone();
        let mut m = RunManager::with_sink(Box::new(shared));
        let (mut cfg, spec) = sh_config("s1", "echo ok");
        cfg.env = vec![("API_KEY".into(), "sup3r-secreto-xyz".into())];
        let spec = spec.env("API_KEY", "sup3r-secreto-xyz");
        let id = launch(&mut m, cfg, spec);
        wait_state(&mut m, id, SessionState::Finished, Duration::from_secs(10));
        let lines = reader.0.lock().unwrap().clone();
        let joined = lines.join("\n");
        assert!(
            !joined.contains("sup3r-secreto-xyz"),
            "log vaza segredo:\n{joined}"
        );
    }

    #[test]
    fn failed_spawn_is_state_not_panic() {
        let mut m = RunManager::new();
        let (cfg, _spec) = sh_config("s1", "true");
        let spec = CommandSpec::new("phpide-inexistente-xyz-123", vec![]);
        let id = launch(&mut m, cfg, spec);
        m.poll();
        let s = m.session(id).unwrap();
        assert_eq!(s.state, SessionState::Failed);
        assert!(s.output().contains("phpide-inexistente-xyz-123"));
    }

    #[test]
    fn reclaim_keeps_pinned_and_recent() {
        let mut m = RunManager::new();
        let mut ids = Vec::new();
        for i in 0..5 {
            let (cfg, spec) = sh_config(&format!("s{i}"), "true");
            ids.push(launch(&mut m, cfg, spec));
        }
        for &id in &ids {
            wait_state(&mut m, id, SessionState::Finished, Duration::from_secs(10));
        }
        // Pina a sessão 0 e reclama com teto 2.
        m.session_mut(ids[0]).unwrap().pinned = true;
        m.reclaim_finished(2);
        assert!(m.session(ids[0]).is_some(), "pinada fica");
        assert!(
            m.sessions().len() <= 3,
            "teto de abas não pinadas: {:?}",
            m.sessions().len()
        );
    }
}
