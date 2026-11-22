//! PTY sessions for the editor's terminal pane.
//!
//! Bytes from the socket are written to the shell. Bytes from the shell are kept
//! so a pane can reattach to the same session.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use thiserror::Error;
use tokio::sync::broadcast;

#[derive(Debug, Error)]
pub enum TerminalError {
    #[error("{0}")]
    Message(String),
}

struct Session {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    output: Arc<Mutex<Vec<u8>>>,
    live: broadcast::Sender<Vec<u8>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    reader: Mutex<Option<std::thread::JoinHandle<()>>>,
}

pub struct Hub {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

impl Default for Hub {
    fn default() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

impl Hub {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.sessions.lock().expect("terminal").len()
    }

    pub fn open(
        &self,
        cwd: &Path,
        program: &str,
        args: &[String],
        cols: u16,
        rows: u16,
    ) -> Result<String, TerminalError> {
        // ConPTY's console host inherits the parent's pipes. Under `cargo test`
        // those pipes are the captured stdout, and CreatePseudoConsole waits
        // forever. Clearing the inherit flag lets the host start.
        detach_stdio_from_children();
        let system = native_pty_system();
        let pair = system
            .openpty(PtySize {
                rows: rows.max(1),
                cols: cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|err| TerminalError::Message(err.to_string()))?;
        let mut command = CommandBuilder::new(program);
        command.cwd(cwd);
        for arg in args {
            command.arg(arg);
        }
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|err| TerminalError::Message(err.to_string()))?;
        let writer = Arc::new(Mutex::new(
            pair.master
                .take_writer()
                .map_err(|err| TerminalError::Message(err.to_string()))?,
        ));
        let (live, _) = broadcast::channel(64);
        let output = Arc::new(Mutex::new(Vec::new()));
        let live_thread = live.clone();
        let output_thread = Arc::clone(&output);
        let reply_writer = Arc::clone(&writer);
        let reader = std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let mut stash = Vec::new();
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let chunk = buf[..count].to_vec();
                        stash.extend_from_slice(&chunk);
                        while let Some(pos) = stash.windows(4).position(|window| window == *b"\x1b[6n")
                        {
                            // ConPTY waits on this cursor report before it draws the shell.
                            let _ = reply_writer
                                .lock()
                                .expect("terminal writer")
                                .write_all(b"\x1b[1;1R");
                            stash.drain(..pos + 4);
                        }
                        if stash.len() > 8 {
                            stash.drain(..stash.len() - 8);
                        }
                        let mut saved = output_thread.lock().expect("terminal output");
                        saved.extend_from_slice(&chunk);
                        if saved.len() > 64 * 1024 {
                            let extra = saved.len() - 64 * 1024;
                            saved.drain(..extra);
                        }
                        drop(saved);
                        let _ = live_thread.send(chunk);
                    }
                }
            }
        });
        // A pending read has to exist before the console host will deliver output.
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|err| TerminalError::Message(err.to_string()))?;
        let _ = pair.master.resize(PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        });
        let id = format!(
            "term-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        );
        self.sessions.lock().expect("terminal").insert(
            id.clone(),
            Arc::new(Session {
                writer,
                master: Mutex::new(pair.master),
                output,
                live,
                child: Mutex::new(child),
                reader: Mutex::new(Some(reader)),
            }),
        );
        Ok(id)
    }

    pub fn write(&self, id: &str, bytes: &[u8]) -> Result<(), TerminalError> {
        let session = self.session(id)?;
        let mut writer = session.writer.lock().expect("terminal writer");
        writer
            .write_all(bytes)
            .map_err(|err| TerminalError::Message(err.to_string()))
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), TerminalError> {
        let session = self.session(id)?;
        let master = session.master.lock().expect("terminal size");
        master
            .resize(PtySize {
                rows: rows.max(1),
                cols: cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|err| TerminalError::Message(err.to_string()))
    }

    pub fn output(&self, id: &str) -> Result<Vec<u8>, TerminalError> {
        let session = self.session(id)?;
        let output = session.output.lock().expect("terminal output");
        Ok(output.clone())
    }

    pub fn subscribe(&self, id: &str) -> Result<broadcast::Receiver<Vec<u8>>, TerminalError> {
        Ok(self.session(id)?.live.subscribe())
    }

    pub fn close(&self, id: &str) -> Result<(), TerminalError> {
        let session = self
            .sessions
            .lock()
            .expect("terminal")
            .remove(id)
            .ok_or_else(|| TerminalError::Message("no such terminal".into()))?;
        let _ = session.child.lock().expect("terminal child").kill();
        // Closing stdin lets a shell that ignored the kill notice see EOF.
        let writer = std::mem::replace(
            &mut *session.writer.lock().expect("terminal writer"),
            Box::new(std::io::sink()),
        );
        drop(writer);
        let reader = session.reader.lock().expect("terminal reader").take();
        if let Some(reader) = reader {
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = reader.join();
                let _ = done_tx.send(());
            });
            if done_rx.recv_timeout(Duration::from_secs(2)).is_err() {
                // The read is still blocked in the PTY. Dropping the console
                // from here deadlocks with that read, so leave the session up.
                std::mem::forget(session);
                return Ok(());
            }
        }
        Ok(())
    }

    fn session(&self, id: &str) -> Result<Arc<Session>, TerminalError> {
        self.sessions
            .lock()
            .expect("terminal")
            .get(id)
            .cloned()
            .ok_or_else(|| TerminalError::Message("no such terminal".into()))
    }
}

fn detach_stdio_from_children() {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        extern "system" {
            fn SetHandleInformation(handle: *mut core::ffi::c_void, mask: u32, flags: u32) -> i32;
        }
        const HANDLE_FLAG_INHERIT: u32 = 1;
        unsafe {
            for handle in [
                std::io::stdin().as_raw_handle(),
                std::io::stdout().as_raw_handle(),
                std::io::stderr().as_raw_handle(),
            ] {
                SetHandleInformation(handle as *mut core::ffi::c_void, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

pub fn default_shell() -> (String, Vec<String>) {
    if cfg!(windows) {
        ("cmd.exe".into(), Vec::new())
    } else {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        (shell, Vec::new())
    }
}

/// Split a configured command on spaces, keeping double-quoted parts together.
pub fn split_command(command: &str) -> (String, Vec<String>) {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in command.chars() {
        match ch {
            '"' => quoted = !quoted,
            ' ' | '\t' if !quoted => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            other => current.push(other),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    let Some(program) = parts.first() else {
        return default_shell();
    };
    (program.clone(), parts.into_iter().skip(1).collect())
}

pub fn wait_for_output(hub: &Hub, id: &str, needle: &str, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Ok(bytes) = hub.output(id) {
            if String::from_utf8_lossy(&bytes).contains(needle) {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shell_command_shows_up_in_the_session_output() {
        let (program, args): (String, Vec<String>) = if cfg!(windows) {
            (
                "cmd.exe".to_string(),
                vec!["/c".into(), "echo".into(), "hello-boggled".into()],
            )
        } else {
            (
                "sh".to_string(),
                vec!["-c".into(), "echo hello-boggled".into()],
            )
        };
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let hub = Hub::new();
            let cwd = std::env::temp_dir();
            let id = hub.open(&cwd, &program, &args, 80, 24).expect("open pty");
            let saw = wait_for_output(&hub, &id, "hello-boggled", Duration::from_secs(5));
            let output = hub.output(&id).unwrap_or_default();
            let _ = hub.close(&id);
            let _ = done_tx.send((saw, output));
        });
        match done_rx.recv_timeout(Duration::from_secs(12)) {
            Ok((true, _)) => {}
            Ok((false, output)) => panic!(
                "the shell did not print hello-boggled: {}",
                String::from_utf8_lossy(&output)
            ),
            Err(_) => panic!("opening a terminal did not finish"),
        }
    }
}
