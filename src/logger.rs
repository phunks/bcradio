use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::{self, rolling::daily};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Registry};
pub struct Logger {
    _guard: WorkerGuard,
}

impl Logger {
    pub fn build(verbosity: u8) -> Self {
        let file_appender = daily(std::env::temp_dir(), "debug_bcradio.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| default_filter(verbosity).into());

        let file_layer = fmt::layer()
            .json()
            .with_writer(non_blocking)
            .with_ansi(false)
            .with_target(true)
            .with_file(true)
            .with_line_number(true)
            .with_level(true)
            .with_thread_ids(verbosity > 3)
            .with_thread_names(verbosity > 4);

        Registry::default()
            .with(EnvFilter::new(filter))
            .with(file_layer)
            .init();

        Self { _guard: guard }
    }
}

/// Debug formatting retains anyhow's cause chain and an enabled backtrace.
#[track_caller]
pub(crate) fn report_error(error: &anyhow::Error) {
    let caller = std::panic::Location::caller();
    tracing::error!(
        error = ?error,
        source_file = caller.file(),
        source_line = caller.line(),
        source_column = caller.column(),
        "operation failed"
    );
}

fn default_filter(verbosity: u8) -> &'static str {
    match verbosity {
        0 => "warn,bcradio=warn",
        1 => "warn,bcradio=info",
        2 => "warn,bcradio=debug",
        _ => "warn,bcradio=trace",
    }
}

#[cfg(test)]
mod tests {
    use super::default_filter;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct TestWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for TestWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn error_log_contains_cause_chain_and_reporting_location() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let writer = TestWriter(output.clone());
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_file(true)
            .with_line_number(true)
            .with_writer(move || writer.clone())
            .finish();
        let error = anyhow::anyhow!("missing field `band_id`").context("Discover API response");
        tracing::subscriber::with_default(subscriber, || super::report_error(&error));
        let bytes = output.lock().unwrap();
        let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let fields = &record["fields"];
        let report = fields["error"].as_str().unwrap();
        assert!(report.contains("Discover API response"));
        assert!(report.contains("missing field `band_id`"));
        if error.backtrace().status() == std::backtrace::BacktraceStatus::Captured {
            assert!(report.contains("Stack backtrace:"));
        }
        assert_eq!(fields["source_file"], file!());
        assert!(fields["source_line"].as_u64().unwrap() > 0);
        assert_eq!(record["level"], "ERROR");
        assert!(record["filename"].as_str().unwrap().ends_with("logger.rs"));
        assert!(record["line_number"].as_u64().unwrap() > 0);
    }

    #[test]
    fn verbosity_levels() {
        assert_eq!(default_filter(0), "warn,bcradio=warn");
        assert_eq!(default_filter(1), "warn,bcradio=info");
        assert_eq!(default_filter(2), "warn,bcradio=debug");
        assert_eq!(default_filter(3), "warn,bcradio=trace");
    }
}
