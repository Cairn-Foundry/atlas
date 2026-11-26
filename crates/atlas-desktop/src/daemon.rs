// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! How the desktop shell starts the Atlas daemon of this machine.
//!
//! The daemon is a process of its own, detached from the shell: closing the window, or quitting
//! the shell, leaves the agents at work. Only an explicit stop ends it.

use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const READY_POLL_INTERVAL: Duration = Duration::from_millis(250);
const LOG_FILE: &str = "daemon.log";
/// Path of the daemon to start instead of the one shipped next to the shell, for development.
const DAEMON_BINARY_VARIABLE: &str = "ATLAS_DAEMON_BIN";

pub fn log_path(home: &Path) -> PathBuf {
    home.join(LOG_FILE)
}

/// The daemon shipped with the shell: Tauri installs a sidecar next to the application's executable.
pub fn daemon_binary() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os(DAEMON_BINARY_VARIABLE) {
        return Ok(PathBuf::from(path));
    }
    let shell = std::env::current_exe()?;
    let name = if cfg!(windows) {
        "atlas-daemon.exe"
    } else {
        "atlas-daemon"
    };
    Ok(shell.with_file_name(name))
}

/// Starts the daemon in a session of its own, its output appended to `daemon.log`: it outlives the shell.
pub fn spawn_detached(binary: &Path, home: &Path) -> io::Result<Child> {
    std::fs::create_dir_all(home)?;
    let log = OpenOptions::new().create(true).append(true).open(log_path(home))?;
    let mut command = Command::new(binary);
    command
        .env("ATLAS_HOME", home)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe and touches nothing of the parent's memory.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    command
        .spawn()
        .map_err(|error| io::Error::new(error.kind(), format!("{} : {error}", binary.display())))
}

/// Waits for the daemon to answer; false when `daemon` died or `timeout` passed first.
pub fn wait_until_running(port: u16, daemon: &mut Child, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if atlas_link::is_running(port) {
            return true;
        }
        if matches!(daemon.try_wait(), Ok(Some(_))) {
            return false;
        }
        std::thread::sleep(READY_POLL_INTERVAL);
    }
    false
}

/// Waits for the daemon to be gone; false when it still answers once `timeout` has passed.
pub fn wait_until_stopped(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !atlas_link::is_running(port) {
            return true;
        }
        std::thread::sleep(READY_POLL_INTERVAL);
    }
    false
}

/// What the shell does about the daemon it finds, or not, when it needs one.
#[derive(Debug, PartialEq, Eq)]
pub enum StartupPlan {
    /// The daemon that runs is the one this shell ships.
    Reuse,
    /// Nothing runs.
    Start,
    /// A daemon of another version runs, left by the application before its update: its API may
    /// not be the one this page speaks.
    Replace,
}

pub fn startup_for(running_version: Option<&str>, shipped_version: &str) -> StartupPlan {
    match running_version {
        None => StartupPlan::Start,
        Some(version) if version == shipped_version => StartupPlan::Reuse,
        Some(_) => StartupPlan::Replace,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpListener};

    use super::*;

    /// Answers every health check on a free port the way a daemon does, until the test ends.
    fn serve_health() -> u16 {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
        let port = listener.local_addr().expect("address").port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request);
                let body = r#"{"name":"atlas","version":"0.1.0"}"#;
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        port
    }

    fn free_port() -> u16 {
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("bind")
            .local_addr()
            .expect("address")
            .port()
    }

    #[test]
    fn reuses_the_shipped_daemon_starts_a_missing_one_and_replaces_another_version() {
        assert_eq!(startup_for(Some("0.1.0"), "0.1.0"), StartupPlan::Reuse);
        assert_eq!(startup_for(None, "0.1.0"), StartupPlan::Start);
        assert_eq!(startup_for(Some("0.0.9"), "0.1.0"), StartupPlan::Replace);
        assert_eq!(startup_for(Some(""), "0.1.0"), StartupPlan::Replace);
    }

    #[test]
    fn starts_the_daemon_with_its_home_and_its_output_in_the_log() {
        let home = tempfile::tempdir().expect("home");
        let binary = home.path().join("daemon");
        std::fs::write(&binary, "#!/bin/sh\necho \"home=$ATLAS_HOME\"\n").expect("script");
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).expect("mode");
        }
        let workspace = home.path().join("atlas");
        let mut child = spawn_detached(&binary, &workspace).expect("spawn");
        assert!(child.wait().expect("wait").success());
        assert_eq!(
            std::fs::read_to_string(log_path(&workspace)).expect("log"),
            format!("home={}\n", workspace.display())
        );
    }

    #[test]
    fn names_the_daemon_it_could_not_start() {
        let home = tempfile::tempdir().expect("home");
        let missing = home.path().join("nowhere");
        let error = spawn_detached(&missing, home.path()).expect_err("error");
        assert!(error.to_string().starts_with(&missing.display().to_string()), "{error}");
    }

    #[test]
    fn waits_for_the_daemon_to_answer_and_gives_up_as_soon_as_it_dies() {
        let mut alive = Command::new("sleep").arg("5").spawn().expect("sleep");
        assert!(wait_until_running(serve_health(), &mut alive, Duration::from_secs(5)));
        alive.kill().expect("kill");
        alive.wait().expect("wait");

        let mut dead = Command::new("true").spawn().expect("true");
        let started = Instant::now();
        assert!(!wait_until_running(free_port(), &mut dead, Duration::from_secs(30)));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn tells_a_daemon_that_stopped_from_one_that_still_answers() {
        assert!(wait_until_stopped(free_port(), Duration::from_secs(5)));
        assert!(!wait_until_stopped(serve_health(), Duration::from_millis(300)));
    }
}
