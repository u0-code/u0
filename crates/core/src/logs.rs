//! What Ghost logs, and where.

/// What Ghost logs goes to stderr and to a file a day in its home's
/// `logs/`, of which the last `days` are kept.
pub fn start(days: u16) {
    use tracing_subscriber::{fmt, layer::SubscriberExt as _, util::SubscriberInitExt as _};
    let file = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("ghost")
        .filename_suffix("log")
        .max_log_files(usize::from(days.max(1)))
        .build(crate::settings::home().join("logs"))
        .map_err(|e| eprintln!("Ghost keeps no log file: {e}"))
        .ok();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ghost_desk=info,ghost_core=info".into()),
        )
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(file.map(|file| fmt::layer().with_ansi(false).with_writer(file)))
        .init();
}
