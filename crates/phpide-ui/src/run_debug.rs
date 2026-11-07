//! Painéis Run / Debug / Test e cobertura — Estágio 8 (RUN-001/002,
//! RUN-DBG-001/002/003, RUN-TST-001/002/003).
//!
//! Consome os domínios `phpide-run` e `phpide-dbgp`: a tool window Run lista
//! as sessões (process list) com Rerun/Stop/Pause Output/Pin; a janela Debug
//! mostra frames, variáveis, watches, evaluate e a toolbar de stepping; a
//! árvore de testes e a cobertura aparecem na sessão de teste selecionada.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use phpide_dbgp::breakpoint::BreakpointStore;
use phpide_dbgp::engine::{DebugEngine, EngineCommand, EngineEvent};
use phpide_dbgp::listener::{xdebug_env, ListenerConfig, DEFAULT_IDE_KEY};
use phpide_dbgp::mapping::PathMappings;
use phpide_dbgp::proto::DEFAULT_PORT;
use phpide_dbgp::session::{ContinueCommand, Frame, StopReason, Variable, Watch};
use phpide_run::config::{RunConfig, RunKind};
use phpide_run::coverage::{import_clover_file, CoverageReport};
use phpide_run::manager::RunManager;
use phpide_run::php_script::{build_command, BuildContext};
use phpide_run::phpunit::{TeamCityParser, TestTree};
use phpide_run::session::SessionId;

/// Estado de debug da IDE (status da sessão DBGp).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugStatus {
    /// Sem listener nem sessão.
    Idle,
    /// Listener ativo aguardando o Xdebug conectar.
    Listening,
    /// Script rodando (após Resume).
    Running,
    /// Parado em breakpoint/step — inspeção disponível.
    Paused,
}

impl DebugStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            DebugStatus::Idle => "idle",
            DebugStatus::Listening => "listening",
            DebugStatus::Running => "running",
            DebugStatus::Paused => "paused",
        }
    }

    /// Sessão ativa (listener ou script vivo) — atalhos de debug roteiam.
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            DebugStatus::Listening | DebugStatus::Running | DebugStatus::Paused
        )
    }
}

/// Estado consolidado dos painéis Run/Debug/Test.
pub struct RunDebugState {
    /// Sessões de execução (process list) com output e estado.
    pub manager: RunManager,
    /// Configurações disponíveis no seletor do painel Run.
    pub configs: Vec<RunConfig>,
    /// Configuração selecionada no seletor (índice).
    pub selected_config: usize,
    /// Sessão selecionada na lista (aba ativa).
    pub selected: Option<SessionId>,
    /// Parsers TeamCity das sessões de teste (alimentados no poll).
    pub test_parsers: HashMap<SessionId, TeamCityParser>,
    /// Árvores finalizadas por sessão de teste.
    pub test_trees: HashMap<SessionId, TestTree>,
    /// Cobertura da última execução de testes com coverage.
    pub coverage: Option<CoverageReport>,

    // Debug (DBGp) — store compartilhado com a engine (mesmo Arc).
    pub bp_store: Arc<Mutex<BreakpointStore>>,
    pub engine: Option<DebugEngine>,
    pub status: DebugStatus,
    /// Sessão do RunManager que roda o PHP em debug (Stop mata o processo).
    pub debug_session: Option<SessionId>,
    pub frames: Vec<Frame>,
    pub selected_frame: usize,
    pub variables: Vec<Variable>,
    /// Último ponto de parada (Show Execution Point / gutter).
    pub last_stop: Option<(PathBuf, u32)>,
    pub watches: Vec<Watch>,
    /// Exibição do último valor por watch — invalidada a cada Resume.
    pub watch_values: Vec<String>,
    pub eval_input: String,
    pub eval_output: String,
    /// Breakpoints mudaram e a engine precisa de UpdateBreakpoints.
    pub breakpoints_dirty: bool,
    /// Quantos bytes do output de cada sessão de teste já foram parseados.
    test_seen: HashMap<SessionId, usize>,
}

impl Default for RunDebugState {
    fn default() -> Self {
        RunDebugState {
            manager: RunManager::new(),
            configs: Vec::new(),
            selected_config: 0,
            selected: None,
            test_parsers: HashMap::new(),
            test_trees: HashMap::new(),
            coverage: None,
            bp_store: Arc::new(Mutex::new(BreakpointStore::new())),
            engine: None,
            status: DebugStatus::Idle,
            debug_session: None,
            frames: Vec::new(),
            selected_frame: 0,
            variables: Vec::new(),
            last_stop: None,
            watches: Vec::new(),
            watch_values: Vec::new(),
            eval_input: String::new(),
            eval_output: String::new(),
            breakpoints_dirty: false,
            test_seen: HashMap::new(),
        }
    }
}

impl RunDebugState {
    /// Linhas 1-based com breakpoint em `path` (gutter do editor).
    pub fn bp_lines_for(&self, path: &Path) -> BTreeSet<usize> {
        self.bp_store
            .lock()
            .unwrap()
            .breakpoints
            .iter()
            .filter(|b| b.file.as_deref() == Some(path))
            .map(|b| b.line as usize)
            .collect()
    }

    /// Status real do breakpoint exatamente nesta linha (gutter pinta
    /// enabled/disabled/verified/invalid — UI-ED-001-04, sem inventar
    /// confirmação do backend).
    pub fn bp_status_for(
        &self,
        path: &Path,
        line1: usize,
    ) -> Option<phpide_dbgp::breakpoint::BreakpointStatus> {
        self.bp_store
            .lock()
            .unwrap()
            .breakpoints
            .iter()
            .find(|b| b.file.as_deref() == Some(path) && b.line as usize == line1)
            .map(|b| b.status.clone())
    }

    /// Mute global dos breakpoints (acinzenta o gutter).
    pub fn bp_muted(&self) -> bool {
        self.bp_store.lock().map(|s| s.muted).unwrap_or(false)
    }

    /// Alterna breakpoint de linha (clique no gutter / Ctrl+F8).
    pub fn toggle_breakpoint(&mut self, path: PathBuf, line: usize) -> bool {
        self.breakpoints_dirty = true;
        self.bp_store.lock().unwrap().toggle_line(path, line as u32)
    }

    /// Porta do listener DBGp (configurável por ambiente).
    pub fn debug_port() -> u16 {
        std::env::var("PHPIDE_DBGP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(DEFAULT_PORT)
    }

    /// Inicia o debug de um script PHP: sobe a engine DBGp e dispara o PHP
    /// com XDEBUG_CONFIG (zero-config, mesclando o env do usuário).
    pub fn start_debug(
        &mut self,
        root: &Path,
        php: &str,
        file: PathBuf,
        interpreter_options: Vec<String>,
    ) -> Result<(), String> {
        if self.status.is_active() {
            return Err("já existe uma sessão de debug ativa".to_string());
        }
        let port = Self::debug_port();
        let config = RunConfig::new(
            "debug-temp",
            format!("debug {}", file.display()),
            RunKind::PhpScript {
                file: file.clone(),
                args: Vec::new(),
                interpreter_options,
                break_at_first_line: false,
            },
        );
        let mut built = build_command(&config, root, &BuildContext::php_only(php))
            .map_err(|e| format!("{} — {}", e.message, e.action))?;
        // XDEBUG_CONFIG: mescla com o valor existente sem apagar chaves do
        // usuário (T-RUN-DBG-001-01).
        let existing = std::env::var("XDEBUG_CONFIG").ok();
        for (k, v) in xdebug_env(port, DEFAULT_IDE_KEY, existing.as_deref()) {
            built.spec = built.spec.env(k, v);
        }
        // break_at_first_line → breakpoint na linha 1 antes de subir a engine.
        if matches!(
            config.kind,
            RunKind::PhpScript {
                break_at_first_line: true,
                ..
            }
        ) {
            self.bp_store.lock().unwrap().add_line(file, 1);
        }
        let engine = DebugEngine::start(
            ListenerConfig {
                port,
                ide_key: Some(DEFAULT_IDE_KEY.to_string()),
                ..Default::default()
            },
            PathMappings::new(root.to_path_buf()),
            self.bp_store.clone(),
        )
        .map_err(|e| format!("falha ao abrir o listener DBGp: {e}"))?;
        self.engine = Some(engine);
        self.status = DebugStatus::Listening;
        let id = self.manager.start(config, built.spec, None);
        self.debug_session = Some(id);
        self.selected = Some(id);
        Ok(())
    }

    /// Drena eventos: sessões de execução + parser de testes + engine DBGp.
    pub fn poll(&mut self) {
        self.manager.poll();
        self.poll_test_parsers();
        self.poll_engine();
        if self.breakpoints_dirty {
            self.breakpoints_dirty = false;
            if let Some(engine) = &self.engine {
                engine.send(EngineCommand::UpdateBreakpoints);
            }
        }
    }

    fn poll_test_parsers(&mut self) {
        let sessions: Vec<SessionId> = self.manager.sessions().iter().map(|s| s.id).collect();
        for id in sessions {
            let Some(session) = self.manager.session(id) else {
                continue;
            };
            if !matches!(session.config.kind, RunKind::PhpUnit { .. }) {
                continue;
            }
            let output_len = session.output().len();
            let seen = self.test_seen.get(&id).copied().unwrap_or(0);
            if output_len > seen {
                let parser = self.test_parsers.entry(id).or_default();
                let full = session.output();
                for line in full[seen..].lines() {
                    parser.feed_line(line);
                }
                self.test_seen.insert(id, full.len());
            }
            if !session.state.is_live() && !self.test_trees.contains_key(&id) {
                if let Some(parser) = self.test_parsers.remove(&id) {
                    let tree = parser.finish();
                    self.test_seen.remove(&id);
                    // Cobertura: importa o Clover quando o plano pediu.
                    if let Some(plan) = &session.plan {
                        if let Some(clover) = &plan.clover_path {
                            if let Ok(report) = import_clover_file(clover) {
                                self.coverage = Some(report);
                            }
                        }
                    }
                    self.test_trees.insert(id, tree);
                }
            }
        }
    }

    fn poll_engine(&mut self) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        for event in engine.poll() {
            match event {
                EngineEvent::Listening { .. } => self.status = DebugStatus::Listening,
                EngineEvent::Connected { .. } => {}
                EngineEvent::BreakpointsSynced(synced) => {
                    // Devolve o store para a UI (estados vêm do backend).
                    self.breakpoints_dirty = false;
                    let _ = synced;
                }
                EngineEvent::Stopped { reason, frames } => {
                    self.status = DebugStatus::Paused;
                    self.frames = frames;
                    self.selected_frame = 0;
                    if let StopReason::Location { file, line } = &reason {
                        self.last_stop = Some((file.clone(), *line));
                    }
                    // Watches são reavaliados a cada parada (RUN-DBG-002);
                    // seus valores anteriores foram invalidados no Continued.
                    self.refresh_watches();
                }
                EngineEvent::FrameVariables(vars) => self.variables = vars,
                EngineEvent::Evaluated { expression, result } => {
                    self.eval_output = match result {
                        Ok(Some(v)) => {
                            let value = v
                                .value
                                .unwrap_or_else(|| format!("<{} vazio>", v.type_name));
                            format!("{expression} = {value}  ({})", v.type_name)
                        }
                        Ok(None) => format!("{expression} = <indisponível neste frame>"),
                        Err(e) => format!("{expression}: erro — {e}"),
                    };
                }
                EngineEvent::WatchValues(values) => {
                    self.watch_values = values
                        .iter()
                        .map(|v| match &v.result {
                            Ok(Some(var)) => var
                                .value
                                .clone()
                                .unwrap_or_else(|| format!("<{} vazio>", var.type_name)),
                            Ok(None) => "<indisponível>".to_string(),
                            Err(e) => format!("erro: {e}"),
                        })
                        .collect();
                }
                EngineEvent::Continued(_) => {
                    self.status = DebugStatus::Running;
                    // Avaliações/watches são inválidas após Resume (RUN-DBG-002).
                    self.watch_values.clear();
                    self.eval_output.clear();
                    self.variables.clear();
                    self.frames.clear();
                }
                EngineEvent::Ended(end) => {
                    self.status = DebugStatus::Idle;
                    let _ = end;
                    if let Some(id) = self.debug_session.take() {
                        self.manager.stop(id);
                    }
                    self.frames.clear();
                    self.variables.clear();
                    self.watch_values.clear();
                    if let Some(engine) = self.engine.take() {
                        drop(engine);
                    }
                }
                EngineEvent::Error(message) => {
                    eprintln!("[debug] {message}");
                }
            }
        }
    }

    /// Comando de continuação (toolbar/atalhos F7/F8/F9/Shift+F8).
    pub fn continue_debug(&mut self, cmd: ContinueCommand) {
        if self.status == DebugStatus::Paused {
            self.status = DebugStatus::Running;
        }
        if let Some(engine) = &self.engine {
            engine.send(EngineCommand::Continue(cmd));
        }
    }

    /// Evaluate Expression (Alt+F8) no frame selecionado.
    pub fn evaluate(&mut self, expression: &str) {
        if expression.trim().is_empty() {
            return;
        }
        if let Some(engine) = &self.engine {
            engine.send(EngineCommand::Evaluate {
                expression: expression.to_string(),
                depth: self.selected_frame as u32,
            });
        }
    }

    /// Reavalia todos os watches no frame selecionado (chamar ao parar).
    pub fn refresh_watches(&mut self) {
        if self.watches.is_empty() {
            return;
        }
        if let Some(engine) = &self.engine {
            engine.send(EngineCommand::EvaluateWatches {
                ids: self.watches.iter().map(|w| w.id).collect(),
                expressions: self.watches.iter().map(|w| w.expression.clone()).collect(),
                depth: self.selected_frame as u32,
            });
        }
    }

    /// Seleção de frame: variáveis passam a vir daquele frame.
    pub fn select_frame(&mut self, depth: usize) {
        self.selected_frame = depth;
        if let Some(engine) = &self.engine {
            engine.send(EngineCommand::SelectFrame(depth as u32));
        }
    }

    /// Detach: engine segue; processo PHP segue (ou termina) sem debug.
    pub fn detach(&mut self) {
        if let Some(engine) = &self.engine {
            engine.send(EngineCommand::Detach);
        }
    }

    /// Stop: encerra a sessão DBGp e mata o processo PHP (grupo inteiro).
    pub fn stop(&mut self) {
        if let Some(engine) = &self.engine {
            engine.send(EngineCommand::Stop);
        }
        if let Some(id) = self.debug_session.take() {
            self.manager.stop(id);
        }
    }

    /// Troca de projeto / fechamento da IDE: encerra tudo (NFR-001-04).
    pub fn shutdown(&mut self) {
        self.manager.stop_all();
        if let Some(mut engine) = self.engine.take() {
            engine.shutdown();
        }
        self.status = DebugStatus::Idle;
    }

    pub fn is_listening(&self) -> bool {
        self.status.is_active()
    }

    /// Inicia a configuração selecionada no seletor (botão Run do painel).
    /// Devolve erro acionável para a status bar (NFR-005).
    pub fn start_config(&mut self, root: &Path, ctx: &BuildContext) -> Result<SessionId, String> {
        let Some(config) = self.configs.get(self.selected_config).cloned() else {
            return Err("nenhuma run configuration selecionada".to_string());
        };
        let built = build_command(&config, root, ctx)
            .map_err(|e| format!("{} — {}", e.message, e.action))?;
        let id = self.manager.start(config, built.spec, built.plan);
        self.selected = Some(id);
        Ok(id)
    }
}
