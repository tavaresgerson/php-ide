//! Log estruturado com correlação e redaction — `T-NFR-005-02`/`T-NFR-005-03` (NFR-005).
//!
//! Operações concorrentes precisam de identificadores correlacionáveis:
//! toda linha carrega `job=<id>`, `doc=<path>@<revisão>` e/ou `sess=<id>`
//! conforme o contexto, além de componente e evento. Os valores de campos
//! passam obrigatoriamente por [`crate::redact::redact_text`] — a redaction
//! é estrutural do logger, não opcional do chamador.

use crate::error::Component;
use crate::jobs::JobId;
use crate::redact::redact_text;
use crate::revision::Revision;
use std::fmt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Identificadores de correlação de uma linha de log (NFR-005-02).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Correlation {
    pub job: Option<JobId>,
    pub document: Option<(PathBuf, Revision)>,
    pub session: Option<u64>,
}

impl Correlation {
    pub fn none() -> Self {
        Correlation::default()
    }

    pub fn job(id: JobId) -> Self {
        Correlation {
            job: Some(id),
            ..Default::default()
        }
    }

    pub fn document(path: impl Into<PathBuf>, revision: Revision) -> Self {
        Correlation {
            document: Some((path.into(), revision)),
            ..Default::default()
        }
    }

    pub fn session(id: u64) -> Self {
        Correlation {
            session: Some(id),
            ..Default::default()
        }
    }

    pub fn with_job(mut self, id: JobId) -> Self {
        self.job = Some(id);
        self
    }

    pub fn with_document(mut self, path: impl Into<PathBuf>, revision: Revision) -> Self {
        self.document = Some((path.into(), revision));
        self
    }

    pub fn with_session(mut self, id: u64) -> Self {
        self.session = Some(id);
        self
    }
}

impl fmt::Display for Correlation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut wrote = false;
        if let Some(job) = self.job {
            write!(f, "job={job}")?;
            wrote = true;
        }
        if let Some((path, rev)) = &self.document {
            if wrote {
                f.write_str(" ")?;
            }
            write!(f, "doc={}@{rev}", path.display())?;
            wrote = true;
        }
        if let Some(sess) = self.session {
            if wrote {
                f.write_str(" ")?;
            }
            write!(f, "sess={sess}")?;
            wrote = true;
        }
        if !wrote {
            f.write_str("-")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    pub const fn as_str(self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

/// Destino das linhas de log.
pub trait LogSink: Send {
    fn write_line(&self, line: &str);
}

/// stderr (default de desenvolvimento; `PHPIDE_LOG=<arquivo>` redireciona).
pub struct StderrSink;

impl LogSink for StderrSink {
    fn write_line(&self, line: &str) {
        eprintln!("{line}");
    }
}

/// Sink em memória para testes.
#[derive(Default)]
pub struct MemorySink {
    lines: Mutex<Vec<String>>,
}

impl MemorySink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lines(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }

    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.lines.lock().unwrap())
    }
}

impl LogSink for MemorySink {
    fn write_line(&self, line: &str) {
        self.lines.lock().unwrap().push(line.to_string());
    }
}

/// Logger mínimo com nível, componente, correlação e redaction obrigatória.
pub struct Logger {
    sink: Box<dyn LogSink>,
    min_level: Level,
}

impl Logger {
    pub fn new(sink: Box<dyn LogSink>) -> Self {
        Logger {
            sink,
            min_level: Level::Info,
        }
    }

    pub fn stderr() -> Self {
        // PHPIDE_LOG=<caminho> direciona para arquivo (append) quando útil
        // para depurar sessões gráficas.
        if let Ok(path) = std::env::var("PHPIDE_LOG") {
            if !path.is_empty() {
                if let Ok(file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                {
                    use std::io::Write;
                    struct FileSink(std::fs::File);
                    impl LogSink for FileSink {
                        fn write_line(&self, line: &str) {
                            let mut f = &self.0;
                            let _ = writeln!(f, "{line}");
                        }
                    }
                    return Logger::new(Box::new(FileSink(file)));
                }
            }
        }
        Logger::new(Box::new(StderrSink))
    }

    pub fn with_min_level(mut self, level: Level) -> Self {
        self.min_level = level;
        self
    }

    /// Emite uma linha. `event` é um nome estável (snake_case); `fields`
    /// são pares chave=valor (valores redigidos automaticamente).
    pub fn log(
        &self,
        level: Level,
        component: Component,
        corr: &Correlation,
        event: &str,
        fields: &[(&str, &str)],
    ) {
        if level < self.min_level {
            return;
        }
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let fields_str = fields
            .iter()
            .map(|(k, v)| format!("{k}={}", redact_text(v)))
            .collect::<Vec<_>>()
            .join(" ");
        let line = if fields_str.is_empty() {
            format!("{ts} {} {component} {corr} {event}", level.as_str())
        } else {
            format!(
                "{ts} {} {component} {corr} {event} {fields_str}",
                level.as_str()
            )
        };
        self.sink.write_line(&line);
    }

    pub fn info(
        &self,
        component: Component,
        corr: &Correlation,
        event: &str,
        fields: &[(&str, &str)],
    ) {
        self.log(Level::Info, component, corr, event, fields);
    }

    pub fn warn(
        &self,
        component: Component,
        corr: &Correlation,
        event: &str,
        fields: &[(&str, &str)],
    ) {
        self.log(Level::Warn, component, corr, event, fields);
    }

    pub fn error(
        &self,
        component: Component,
        corr: &Correlation,
        event: &str,
        fields: &[(&str, &str)],
    ) {
        self.log(Level::Error, component, corr, event, fields);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correlation_renders_all_ids() {
        let c = Correlation::job(JobId(7))
            .with_document("/p/a.php", 42)
            .with_session(99);
        assert_eq!(c.to_string(), "job=#7 doc=/p/a.php@42 sess=99");
        assert_eq!(Correlation::none().to_string(), "-");
        assert_eq!(Correlation::session(1).to_string(), "sess=1");
    }

    /// Sink compartilhado entre o logger (movido para dentro) e o teste.
    struct SharedSink(std::sync::Arc<MemorySink>);

    impl LogSink for SharedSink {
        fn write_line(&self, line: &str) {
            self.0.write_line(line);
        }
    }

    fn logger_with(sink: &std::sync::Arc<MemorySink>) -> Logger {
        Logger::new(Box::new(SharedSink(sink.clone())))
    }

    #[test]
    fn log_line_contains_ids_event_and_fields() {
        let sink = std::sync::Arc::new(MemorySink::new());
        let log = logger_with(&sink);
        let corr = Correlation::job(JobId(3)).with_document("/x.php", 5);
        log.info(Component::Lsp, &corr, "did_change", &[("bytes", "128")]);
        let lines = sink.lines();
        assert_eq!(lines.len(), 1);
        let l = &lines[0];
        assert!(l.contains("INFO lsp"), "{l}");
        assert!(l.contains("job=#3"), "{l}");
        assert!(l.contains("doc=/x.php@5"), "{l}");
        assert!(l.contains("did_change"), "{l}");
        assert!(l.contains("bytes=128"), "{l}");
    }

    #[test]
    fn log_values_are_redacted_and_level_filters() {
        let sink = std::sync::Arc::new(MemorySink::new());
        let log = logger_with(&sink).with_min_level(Level::Warn);
        log.info(
            Component::Exec,
            &Correlation::none(),
            "spawn",
            &[("cmd", "php")],
        );
        assert!(sink.lines().is_empty(), "abaixo do nível é descartado");
        log.warn(
            Component::Exec,
            &Correlation::session(4),
            "spawn",
            &[("env", "DB_PASSWORD=hunter2 TOKEN=abc"), ("cmd", "php -v")],
        );
        let lines = sink.lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("WARN exec"), "{}", lines[0]);
        assert!(lines[0].contains("DB_PASSWORD=***"), "{}", lines[0]);
        assert!(lines[0].contains("TOKEN=***"), "{}", lines[0]);
        assert!(lines[0].contains("cmd=php -v"), "{}", lines[0]);
        assert!(lines[0].contains("sess=4"), "{}", lines[0]);
    }
}
