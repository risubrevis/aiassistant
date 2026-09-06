use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter};
use tracing::{error, info};

use super::{load, Config};

/// Spawn a background thread that reloads `config.toml` on external edits,
/// updates the shared in-memory config, and emits `config:reloaded` to the
/// frontend. Debounced to coalesce editor save-flurries.
pub fn spawn_config_watcher(app: AppHandle, config: Arc<RwLock<Config>>) {
    let path = super::config_path();
    std::thread::spawn(move || run_watcher(app, config, &path));
}

fn run_watcher(app: AppHandle, config: Arc<RwLock<Config>>, path: &Path) {
    let (tx, rx) = mpsc::channel();
    let mut watcher = match RecommendedWatcher::new(
        move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                let _ = tx.send(ev);
            }
        },
        notify::Config::default(),
    ) {
        Ok(w) => w,
        Err(e) => {
            error!("failed to create config watcher: {e}");
            return;
        }
    };

    if let Err(e) = watcher.watch(path, RecursiveMode::NonRecursive) {
        error!("failed to watch {}: {}", path.display(), e);
        return;
    }

    info!("watching {} for changes", path.display());

    let mut last = Instant::now() - Duration::from_secs(1);
    for ev in rx {
        if !matches!(
            ev.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
        ) {
            continue;
        }
        let now = Instant::now();
        if now.duration_since(last) < Duration::from_millis(200) {
            continue;
        }
        last = now;
        match load() {
            Ok(cfg) => {
                crate::net::init(&cfg.network);
                if let Ok(mut w) = config.write() {
                    *w = cfg;
                }
                if let Err(e) = app.emit("config:reloaded", ()) {
                    error!("failed to emit config:reloaded: {e}");
                }
            }
            Err(e) => error!("config hot-reload failed: {e}"),
        }
    }
}
