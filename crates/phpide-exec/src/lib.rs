//! phpide-exec — execução de processos externos e descoberta de ferramentas.
//!
//! Implementa `NFR-003` (T-NFR-003-01…04):
//!
//! - [`discover`]: descoberta configurável de executáveis com diagnóstico de
//!   origem (configuração explícita → PATH) e verificação de versão;
//! - [`CommandSpec`]/[`run`]: spawn padronizado — argumentos como vetor
//!   (sem shell, exceto Terminal explícito), cwd, env, stdout/stderr
//!   separados e exit code;
//! - timeout, Stop cooperativo por cancelamento de job e limpeza de
//!   processos-filhos (grupo de processos no Unix);
//! - mensagens acionáveis para executável ausente, permissão negada,
//!   versão incompatível e caminhos com espaços (NFR-005).

mod discover;
mod process;
mod stream;

pub use discover::{discover, ToolInfo, ToolRequest};
pub use process::{kill_process_tree, run, spawn_job, CommandSpec, Completed};
pub use stream::{spawn_stream, StreamEvent, StreamingChild};
