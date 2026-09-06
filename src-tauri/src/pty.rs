use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use portable_pty::{CommandBuilder, PtySize};
use tokio::sync::mpsc;
use tracing::warn;

/// A running PTY session: writer for input, child to await exit.
struct Session {
    writer: Mutex<Box<dyn std::io::Write + Send>>,
    killer: Mutex<Option<Box<dyn portable_pty::ChildKiller + Send + Sync>>>,
    child: Mutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>,
}

/// PTY session manager for interactive commands (docs/12: PTY-card for sudo).
#[derive(Clone, Default)]
pub struct PtyManager {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawn a command through the platform shell in a PTY. Returns (session_id, output receiver).
    pub fn spawn(
        &self,
        command: &str,
        cwd: Option<&std::path::Path>,
        env: &HashMap<String, String>,
    ) -> Result<(String, mpsc::Receiver<Vec<u8>>)> {
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| anyhow!("openpty: {e}"))?;

        let mut cmd = {
            #[cfg(windows)]
            {
                let mut c = CommandBuilder::new("cmd");
                c.arg("/C");
                c.arg(command);
                c
            }
            #[cfg(not(windows))]
            {
                let mut c = CommandBuilder::new("sh");
                c.arg("-c");
                c.arg(command);
                c
            }
        };
        if let Some(c) = cwd {
            if !c.is_dir() {
                return Err(anyhow!("cwd is not a directory: {}", c.display()));
            }
            cmd.cwd(c);
        }
        for (k, v) in env {
            cmd.env(k, v);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| anyhow!("pty spawn: {e}"))?;
        let killer = child.clone_killer();
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| anyhow!("pty reader: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| anyhow!("pty writer: {e}"))?;
        // Drop the slave to let the child see EOF when the master closes.
        drop(pair.slave);
        // Keep master alive for the child's lifetime via the reader thread.
        let _master = pair.master;

        let (tx, rx) = mpsc::channel::<Vec<u8>>(256);
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.blocking_send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("pty read error: {e}");
                        break;
                    }
                }
            }
            drop(_master); // drop master after reader ends to signal EOF/exit
            drop(reader);
        });

        let id = uuid::Uuid::new_v4().to_string();
        self.sessions.lock().unwrap().insert(
            id.clone(),
            Session {
                writer: Mutex::new(writer),
                killer: Mutex::new(Some(killer)),
                child: Mutex::new(Some(child)),
            },
        );
        Ok((id, rx))
    }

    /// Send user input (e.g. sudo password) to the PTY stdin.
    pub fn write_input(&self, session_id: &str, data: &[u8]) -> Result<()> {
        let sessions = self.sessions.lock().unwrap();
        let Some(s) = sessions.get(session_id) else {
            return Err(anyhow!("pty session not found"));
        };
        let mut w = s.writer.lock().unwrap();
        w.write_all(data).map_err(|e| anyhow!("pty write: {e}"))?;
        w.flush().map_err(|e| anyhow!("pty flush: {e}"))?;
        Ok(())
    }

    /// Kill the child process of a session.
    pub fn kill(&self, session_id: &str) -> Result<()> {
        let killer = {
            let sessions = self.sessions.lock().unwrap();
            sessions
                .get(session_id)
                .and_then(|s| s.killer.lock().unwrap().take())
        };
        let Some(mut killer) = killer else {
            return Err(anyhow!("pty session not found"));
        };
        killer.kill().map_err(|e| anyhow!("pty kill: {e}"))
    }

    /// Wait for the child to exit; returns (exit code, human status label).
    pub async fn wait(&self, session_id: &str) -> Result<(i32, String)> {
        let child = {
            let sessions = self.sessions.lock().unwrap();
            sessions
                .get(session_id)
                .and_then(|s| s.child.lock().unwrap().take())
        };
        let Some(mut child) = child else {
            return Err(anyhow!("pty session not found / already waited"));
        };
        let status = tokio::task::spawn_blocking(move || child.wait())
            .await
            .map_err(|e| anyhow!("wait join: {e}"))?
            .map_err(|e| anyhow!("child wait: {e}"))?;
        // portable-pty 0.8.1 keeps the signal name private; its Display renders
        // "Terminated by <signal>" for signal deaths.
        let label = if let Some(sig) = status.to_string().strip_prefix("Terminated by ") {
            format!("signal {sig}")
        } else {
            format!("exit {}", status.exit_code())
        };
        let code = status.exit_code() as i32;
        self.sessions.lock().unwrap().remove(session_id);
        Ok((code, label))
    }
}
