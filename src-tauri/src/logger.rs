use std::path::PathBuf;
use std::sync::Arc;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::{fmt, prelude::*, reload};

/// Applies new console/file log levels at runtime (set_log_level command).
pub type LevelReloader = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Initialize `tracing` to a log file under the OS logs dir (see docs/04).
/// Console and file layers get independent level filters from `logging.level`
/// / `logging.file_level` (config.toml); the returned reloader applies changes
/// live. Returns the log file path, the non-blocking writer guard that must be
/// kept alive for the app's lifetime, and the reloader.
pub fn init() -> (PathBuf, WorkerGuard, LevelReloader) {
    let logs_dir = log_dir();
    let _ = std::fs::create_dir_all(&logs_dir);

    let file_appender = tracing_appender::rolling::never(&logs_dir, "aiassistant.log");
    let log_path = logs_dir.join("aiassistant.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let (level, file_level) = configured_levels();

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_filter(level_filter(&file_level));
    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(level_filter(&level));

    // Wrap each filtered layer so set_log_level can swap levels at runtime.
    let (file_layer, file_handle) = reload::Layer::new(file_layer);
    let (stderr_layer, stderr_handle) = reload::Layer::new(stderr_layer);

    let _ = tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer)
        .try_init();

    let reloader: LevelReloader = Arc::new(move |level: &str, file_level: &str| {
        let f = level_filter(file_level);
        let _ = file_handle.modify(|l| *l.filter_mut() = f);
        let s = level_filter(level);
        let _ = stderr_handle.modify(|l| *l.filter_mut() = s);
    });

    (log_path, guard, reloader)
}

fn level_filter(level: &str) -> LevelFilter {
    match level.trim().to_ascii_lowercase().as_str() {
        "off" => LevelFilter::OFF,
        "error" => LevelFilter::ERROR,
        "warn" => LevelFilter::WARN,
        "debug" => LevelFilter::DEBUG,
        "trace" => LevelFilter::TRACE,
        _ => LevelFilter::INFO,
    }
}

/// Read `logging.level` / `logging.file_level` from config.toml without the
/// side effects of config::load (this runs before config has been loaded).
fn configured_levels() -> (String, String) {
    let defaults = crate::config::Config::default().logging;
    let fallback = (defaults.level.clone(), defaults.file_level.clone());
    let Ok(raw) = std::fs::read_to_string(crate::config::config_path()) else {
        return fallback;
    };
    let Ok(doc) = raw.parse::<toml::Table>() else {
        return fallback;
    };
    let get = |key: &str| {
        doc.get("logging")
            .and_then(|l| l.get(key))
            .and_then(toml::Value::as_str)
            .map(str::to_string)
    };
    let file_level = get("file_level").unwrap_or(defaults.file_level);
    let level = get("level").unwrap_or(defaults.level);
    (level, file_level)
}

fn log_dir() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("aiassistant")
        .join("logs")
}
