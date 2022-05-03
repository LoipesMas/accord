use flexi_logger::{writers::LogWriter, DeferredNow, FormatFunction};
use log::Record;
use tokio::sync::mpsc;

pub struct LogEntry {
    pub level: log::Level,
    pub target: String,
    pub args: String,
}

impl From<&Record<'_>> for LogEntry {
    fn from(record: &Record) -> Self {
        Self {
            level: record.level(),
            target: record.target().to_string(),
            args: record.args().to_string(),
        }
    }
}

pub struct LogVec {
    logs_tx: mpsc::Sender<LogEntry>,
}

impl LogVec {
    pub fn new(logs_tx: mpsc::Sender<LogEntry>) -> Self {
        Self { logs_tx }
    }
}

impl LogWriter for LogVec {
    fn max_log_level(&self) -> log::LevelFilter {
        log::LevelFilter::Trace
    }

    fn format(&mut self, format: FormatFunction) {
        let _ = format;
    }

    fn shutdown(&self) {}

    fn write(&self, _now: &mut DeferredNow, record: &Record) -> std::io::Result<()> {
        let s = record.into();
        self.logs_tx
            .try_send(s)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }

    fn flush(&self) -> std::io::Result<()> {
        Ok(())
    }
}
