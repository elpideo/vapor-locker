use std::{
    fs,
    io,
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use file_rotate::{compression::Compression, suffix::AppendCount, ContentLimit, FileRotate};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub struct LogGuard {
    _keep_alive: Arc<Mutex<FileRotate<AppendCount>>>,
}

pub fn init_logging_from_env() -> anyhow::Result<LogGuard> {
    let log_dir = std::env::var("LOG_DIR").unwrap_or_else(|_| "logs".to_string());
    let log_file = std::env::var("LOG_FILE").unwrap_or_else(|_| "vapor.log".to_string());
    let max_bytes: u64 = std::env::var("LOG_MAX_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100 * 1024 * 1024);
    let max_files: usize = std::env::var("LOG_MAX_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);

    fs::create_dir_all(&log_dir).with_context(|| format!("create log dir {log_dir}"))?;
    let path = Path::new(&log_dir).join(log_file);

    let max_bytes_usize = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    let rotate = FileRotate::new(
        path,
        AppendCount::new(max_files),
        ContentLimit::BytesSurpassed(max_bytes_usize),
        Compression::None,
        None,
    );

    let shared = Arc::new(Mutex::new(rotate));
    let writer = SharedMakeWriter(shared.clone());

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .flatten_event(true)
                .with_current_span(false)
                .with_span_list(false)
                .with_writer(writer),
        )
        .init();

    Ok(LogGuard { _keep_alive: shared })
}

#[derive(Clone)]
struct SharedMakeWriter(Arc<Mutex<FileRotate<AppendCount>>>);

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SharedMakeWriter {
    type Writer = SharedWriterGuard;

    fn make_writer(&'a self) -> Self::Writer {
        SharedWriterGuard(self.0.clone())
    }
}

struct SharedWriterGuard(Arc<Mutex<FileRotate<AppendCount>>>);

impl io::Write for SharedWriterGuard {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut w = self
            .0
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "log mutex poisoned"))?;
        w.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut w = self
            .0
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "log mutex poisoned"))?;
        w.flush()
    }
}

