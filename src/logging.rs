use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub fn init_logging() -> Option<WorkerGuard> {
    // Only enable logging if RUST_LOG is set
    let filter = match std::env::var("RUST_LOG") {
        Ok(f) => f,
        Err(_) => return None,
    };

    // Create log directory - always use ~/.local/state for consistency
    let log_dir = dirs::home_dir()
        .map(|d| d.join(".local/state/claude-permissions"))
        .unwrap_or_else(|| PathBuf::from("/tmp/claude-permissions"));

    if std::fs::create_dir_all(&log_dir).is_err() {
        return None;
    }

    let file_appender = tracing_appender::rolling::daily(&log_dir, "debug.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(EnvFilter::new(filter))
        .with(
            fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_target(false),
        )
        .init();

    Some(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logging_disabled_by_default() {
        // Clear RUST_LOG if set
        std::env::remove_var("RUST_LOG");
        let guard = init_logging();
        assert!(guard.is_none());
    }
}
