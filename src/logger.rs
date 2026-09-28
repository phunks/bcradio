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

    #[test]
    fn verbosity_levels() {
        assert_eq!(default_filter(0), "warn,bcradio=warn");
        assert_eq!(default_filter(1), "warn,bcradio=info");
        assert_eq!(default_filter(2), "warn,bcradio=debug");
        assert_eq!(default_filter(3), "warn,bcradio=trace");
    }
}
