//! Sessão de execução — `T-RUN-001-02` (sessões, cwd, env, output, estado).
//!
//! Uma sessão amarra uma [`RunConfig`] a um processo vivo ([`StreamingChild`])
//! e ao buffer de saída da sua aba. Estados observáveis (NFR-001): Starting →
//! Running ⇄ PausedOutput → Finished/Stopped/Failed. Pause Output congela o
//! buffer (o processo continua rodando); Stop mata o grupo de processos
//! (NFR-003-03) e o estado final é Stopped; exit natural é Finished.

use std::time::Instant;

use phpide_exec::{spawn_stream, CommandSpec, StreamEvent, StreamingChild};

use crate::config::RunConfig;

/// Identificador estável da sessão (aba/process list).
pub type SessionId = u64;

/// Estado observável de uma sessão de execução (NFR-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Spawn disparado, primeiro drain ainda não aconteceu.
    Starting,
    /// Processo rodando, saída sendo anexada.
    Running,
    /// Pause Output: processo vivo, saída congelada no buffer.
    PausedOutput,
    /// Exit natural (exit code disponível).
    Finished,
    /// Encerrada por Stop (sem exit code — morta por sinal).
    Stopped,
    /// Falha de spawn/validação (mensagem no output).
    Failed,
}

impl SessionState {
    pub const fn is_live(self) -> bool {
        matches!(
            self,
            SessionState::Starting | SessionState::Running | SessionState::PausedOutput
        )
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            SessionState::Starting => "starting",
            SessionState::Running => "running",
            SessionState::PausedOutput => "paused-output",
            SessionState::Finished => "finished",
            SessionState::Stopped => "stopped",
            SessionState::Failed => "failed",
        }
    }
}

/// Limite do buffer de saída por sessão (mesmo teto do painel legado).
const MAX_OUTPUT: usize = 400_000;

/// Sessão de execução: configuração + processo + buffer de saída + estado.
pub struct RunSession {
    pub id: SessionId,
    pub config: RunConfig,
    /// Artefatos do plano (ex.: Clover de cobertura do PHPUnit).
    pub plan: Option<crate::phpunit::PhpUnitPlan>,
    /// Spec que originou o processo — guardada para Rerun.
    spec: CommandSpec,
    child: Option<StreamingChild>,
    pub state: SessionState,
    pub started_at: Instant,
    pub exit_code: Option<i32>,
    /// Pin Tab (RUN-001): a aba da sessão não é reciclada.
    pub pinned: bool,
    /// Falha de spawn (estado Failed).
    pub failure: Option<String>,
    output: String,
    truncated: bool,
    stop_requested: bool,
}

impl RunSession {
    /// Spawna a sessão. Falha de spawn vira estado `Failed` com mensagem
    /// acionável no output — nunca panic (NFR-003/NFR-005).
    pub fn start(
        id: SessionId,
        config: RunConfig,
        spec: CommandSpec,
        plan: Option<crate::phpunit::PhpUnitPlan>,
    ) -> Self {
        let started_at = Instant::now();
        let (child, state, failure) = match spawn_stream(spec.clone()) {
            Ok(child) => (Some(child), SessionState::Starting, None),
            Err(e) => (
                None,
                SessionState::Failed,
                Some(format!("{} — ação: {}", e.message, e.action)),
            ),
        };
        // Falha de spawn aparece no output da aba (mensagem acionável).
        let output = failure
            .as_ref()
            .map(|f| {
                format!(
                    "{f}
"
                )
            })
            .unwrap_or_default();
        RunSession {
            id,
            config,
            plan,
            spec,
            child,
            state,
            started_at,
            exit_code: None,
            pinned: false,
            failure,
            output,
            truncated: false,
            stop_requested: false,
        }
    }

    pub fn title(&self) -> String {
        self.config.name.clone()
    }

    /// Linha de comando pública (programa + args, sem env — redaction).
    pub fn command_line(&self) -> String {
        self.child
            .as_ref()
            .map(|c| c.description.clone())
            .unwrap_or_else(|| format!("{} {}", self.spec.program, self.spec.args.join(" ")))
    }

    pub fn output(&self) -> &str {
        &self.output
    }

    pub fn is_running(&self) -> bool {
        self.child.as_ref().is_some_and(StreamingChild::is_running)
    }

    /// Stop solicitado e confirmação de exit ainda não chegou — para efeito
    /// de `allow_multiple_instances`, sessão parando não bloqueia novo start.
    pub fn is_stopping(&self) -> bool {
        self.stop_requested
    }

    /// Drena eventos do processo (chamar a cada frame pela UI/manager).
    pub fn poll(&mut self) {
        let Some(child) = &self.child else {
            return;
        };
        let mut exit = None;
        for ev in child.drain() {
            match ev {
                StreamEvent::Line(line) => self.push_line(&line, false),
                StreamEvent::ErrLine(line) => self.push_line(&line, true),
                StreamEvent::Exit(code) => exit = Some(code),
            }
        }
        if self.state == SessionState::Starting && exit.is_none() {
            self.state = SessionState::Running;
        }
        if let Some(code) = exit {
            self.exit_code = code;
            self.state = if self.stop_requested || code.is_none() {
                SessionState::Stopped
            } else {
                SessionState::Finished
            };
            self.child = None;
        }
    }

    fn push_line(&mut self, line: &str, is_err: bool) {
        // Pause Output: descarta linhas enquanto pausado (processo segue).
        if self.state == SessionState::PausedOutput {
            return;
        }
        if self.output.len() > MAX_OUTPUT {
            self.output.clear();
            self.truncated = true;
            self.output.push_str("[saída truncada]\n");
        }
        if is_err {
            self.output.push_str("[stderr] ");
        }
        self.output.push_str(line);
        self.output.push('\n');
    }

    /// Pause Output (RUN-001): alterna congelamento do buffer.
    pub fn set_output_paused(&mut self, paused: bool) {
        if !self.state.is_live() {
            return;
        }
        self.state = if paused {
            SessionState::PausedOutput
        } else {
            SessionState::Running
        };
    }

    /// Stop: encerra o grupo de processos; estado final chega no próximo
    /// poll como `Stopped` (exit sem código).
    pub fn stop(&mut self) {
        if let Some(child) = &self.child {
            child.stop();
            self.stop_requested = true;
            self.push_line_status("[parado pelo usuário]");
        }
    }

    /// Encerramento por troca de projeto/saída da IDE (NFR-001-04).
    pub fn shutdown(&mut self) {
        if self.state.is_live() {
            self.stop();
            self.poll();
        }
    }

    fn push_line_status(&mut self, line: &str) {
        if self.output.len() <= MAX_OUTPUT {
            self.output.push_str(line);
            self.output.push('\n');
        }
    }

    /// Spec original, para Rerun (RUN-001).
    pub fn spec(&self) -> &CommandSpec {
        &self.spec
    }

    pub fn was_truncated(&self) -> bool {
        self.truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_str_is_stable() {
        assert_eq!(SessionState::Starting.as_str(), "starting");
        assert_eq!(SessionState::PausedOutput.as_str(), "paused-output");
        assert!(SessionState::Running.is_live());
        assert!(!SessionState::Finished.is_live());
    }
}
