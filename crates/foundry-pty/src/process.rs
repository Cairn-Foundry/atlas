// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A command running on a pseudo-terminal.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::Duration;

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, native_pty_system};

use crate::command::resolve_command;

const READ_BUFFER_BYTES: usize = 8192;
const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_millis(500);
/// Writes kept for a process that is not reading its terminal; past it, input is dropped.
const PENDING_WRITES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    pub cols: u16,
    pub rows: u16,
}

impl From<PtySize> for portable_pty::PtySize {
    fn from(size: PtySize) -> Self {
        Self {
            rows: size.rows,
            cols: size.cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpawnRequest {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    /// The whole environment of the process: nothing is inherited besides it.
    pub env: Vec<(String, String)>,
    pub size: PtySize,
}

/// What a process on a PTY tells its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtyEvent {
    /// Bytes the process printed, as they came: a chunk may end in the middle of a character.
    Data(Vec<u8>),
    /// The process ended, with its exit code when the system gave one.
    Exit(Option<i32>),
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PtyError {
    #[error("ouverture du pseudo-terminal impossible : {0}")]
    Open(String),
    #[error("lancement de {command} impossible : {reason}")]
    Spawn { command: String, reason: String },
}

pub struct PtyProcess {
    master: Mutex<Box<dyn MasterPty + Send>>,
    input: mpsc::SyncSender<Vec<u8>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
}

impl PtyProcess {
    /// Types `data` into the terminal without waiting for the process to read it; a process that
    /// is gone takes nothing, and one that stopped reading loses what overflows its queue.
    pub fn write(&self, data: &[u8]) {
        let _ = self.input.try_send(data.to_vec());
    }

    pub fn resize(&self, size: PtySize) {
        let master = self.master.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = master.resize(size.into());
    }

    /// Ends the process, as closing its terminal would.
    pub fn kill(&self) {
        let _ = self.killer.lock().unwrap_or_else(PoisonError::into_inner).kill();
    }
}

/// Starts `request` on a new pseudo-terminal. `on_event` is called from background threads with
/// what the process prints, then once with its exit.
pub fn spawn(
    request: &SpawnRequest,
    on_event: impl Fn(PtyEvent) + Send + Sync + 'static,
) -> Result<PtyProcess, PtyError> {
    let pair = native_pty_system()
        .openpty(request.size.into())
        .map_err(|error| PtyError::Open(error.to_string()))?;
    let resolved = resolve_command(
        &request.command,
        request.env.iter().map(|(key, value)| (key.as_str(), value.as_str())),
    );
    let mut command = CommandBuilder::new(&resolved.file);
    command.args(&resolved.prefix);
    command.args(&request.args);
    command.cwd(&request.cwd);
    command.env_clear();
    for (key, value) in &request.env {
        command.env(key, value);
    }
    command.env("TERM", "xterm-256color");
    let spawn_error = |reason: String| PtyError::Spawn {
        command: request.command.clone(),
        reason,
    };
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| spawn_error(error.to_string()))?;
    drop(pair.slave);
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| spawn_error(error.to_string()))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|error| spawn_error(error.to_string()))?;
    let killer = child.clone_killer();

    // The terminal takes about a kilobyte before a write waits for the process to read: the wait
    // happens here, never on the caller's thread.
    let (input, pending) = mpsc::sync_channel::<Vec<u8>>(PENDING_WRITES);
    std::thread::spawn(move || {
        while let Ok(data) = pending.recv() {
            if writer.write_all(&data).and_then(|()| writer.flush()).is_err() {
                break;
            }
        }
    });

    let on_event = Arc::new(on_event);
    let on_data = Arc::clone(&on_event);
    let (drained, output_drained) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        let mut buffer = [0_u8; READ_BUFFER_BYTES];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                break;
            }
            on_data(PtyEvent::Data(buffer[..count].to_vec()));
        }
        drop(drained);
    });
    std::thread::spawn(move || {
        let code = child
            .wait()
            .ok()
            .and_then(|status| i32::try_from(status.exit_code()).ok());
        // What the process printed last comes before its exit. Bounded: a process it left behind
        // may keep the terminal open for ever.
        let _ = output_drained.recv_timeout(OUTPUT_DRAIN_TIMEOUT);
        on_event(PtyEvent::Exit(code));
    });

    Ok(PtyProcess {
        master: Mutex::new(pair.master),
        input,
        killer: Mutex::new(killer),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    const WAIT: Duration = Duration::from_secs(10);

    fn shell(script: &str) -> SpawnRequest {
        SpawnRequest {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: std::env::temp_dir().to_string_lossy().into_owned(),
            env: vec![
                ("PATH".into(), "/usr/bin:/bin".into()),
                ("ATLAS_MARK".into(), "seen".into()),
            ],
            size: PtySize { cols: 80, rows: 24 },
        }
    }

    /// Everything printed until the exit, and the exit code.
    fn collect(events: &mpsc::Receiver<PtyEvent>) -> (String, Option<i32>) {
        let mut output = Vec::new();
        loop {
            match events.recv_timeout(WAIT).expect("event before the timeout") {
                PtyEvent::Data(data) => output.extend(data),
                PtyEvent::Exit(code) => return (String::from_utf8_lossy(&output).into_owned(), code),
            }
        }
    }

    #[test]
    fn reports_the_output_then_the_exit_code_with_only_the_given_environment() {
        let (sender, events) = mpsc::channel();
        let _process = spawn(&shell("echo \"$ATLAS_MARK:${HOME:-none}\"; exit 3"), move |event| {
            let _ = sender.send(event);
        })
        .expect("spawn");
        let (output, code) = collect(&events);
        assert!(output.contains("seen:none"), "output: {output}");
        assert_eq!(code, Some(3));
    }

    #[test]
    fn types_into_the_terminal_and_reports_its_size() {
        let (sender, events) = mpsc::channel();
        let process = spawn(&shell("read line; echo \"got:$line\"; stty size"), move |event| {
            let _ = sender.send(event);
        })
        .expect("spawn");
        process.resize(PtySize { cols: 100, rows: 30 });
        process.write(b"hello\n");
        let (output, code) = collect(&events);
        assert!(output.contains("got:hello"), "output: {output}");
        assert!(output.contains("30 100"), "output: {output}");
        assert_eq!(code, Some(0));
    }

    #[test]
    fn kills_a_process_that_would_never_end() {
        let (sender, events) = mpsc::channel();
        let process = spawn(&shell("sleep 600"), move |event| {
            let _ = sender.send(event);
        })
        .expect("spawn");
        process.kill();
        let (_, code) = collect(&events);
        assert_ne!(code, Some(0));
    }

    #[test]
    fn refuses_a_command_that_does_not_exist() {
        let request = SpawnRequest {
            command: "/nonexistent/atlas-command".into(),
            ..shell("")
        };
        assert!(matches!(spawn(&request, |_| {}), Err(PtyError::Spawn { .. })));
    }
}
