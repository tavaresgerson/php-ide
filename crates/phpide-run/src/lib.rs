//! phpide-run — execução, testes e cobertura (Estágio 8).
//!
//! Domínio de Run Configurations (RUN-001/002), Test Runner (RUN-TST-001/002)
//! e cobertura (RUN-TST-003). A UI consome estes tipos; aqui não há egui.
//!
//! - [`config`]: modelo versionado + validação (T-RUN-001-01, T-RUN-002-01);
//! - [`session`]: sessões com estado, cwd, env, output e Pause Output
//!   (T-RUN-001-02);
//! - [`manager`]: process list, Rerun, Stop, Pin Tab e redaction de env nos
//!   logs (T-RUN-001-03/05);
//! - [`php_script`]: montagem de comando validada (T-RUN-002-01);
//! - [`phpunit`]: runner, escopos e árvore de resultados (T-RUN-002-03,
//!   T-RUN-TST-001-01/02, T-RUN-TST-002-01);
//! - [`coverage`]: modelo por arquivo/linha e importador Clover
//!   (T-RUN-TST-003-01).

pub mod config;
pub mod coverage;
pub mod manager;
pub mod php_script;
pub mod phpunit;
pub mod session;

pub use config::{
    parse_run_config, IssueSeverity, RunConfig, RunConfigFile, RunKind, TestScope, ValidationIssue,
    RUN_CONFIG_FORMAT_VERSION,
};
pub use coverage::{import_clover, import_clover_file, CoverageReport, FileCoverage};
pub use manager::RunManager;
pub use php_script::{build_command, BuildContext, BuiltCommand};
pub use phpunit::{
    consolidate as consolidate_phpunit, resolve_phpunit, scope_args, PhpUnitPlan, PhpUnitSetup,
    TeamCityParser, TestNode, TestStats, TestStatus, TestTree,
};
pub use session::{RunSession, SessionId, SessionState};
