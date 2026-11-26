// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Installing a project's dependencies in a worktree Atlas just created.

use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

/// An install that has not finished by then is stuck, not slow.
const SETUP_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// How a dependency install ended: its exit code, none when it was killed (timeout) or never started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupResult {
    pub exit_code: Option<i32>,
}

impl SetupResult {
    pub fn is_success(self) -> bool {
        self.exit_code == Some(0)
    }
}

/// Runs a project's setup command in a folder, its output written to `log_path`.
pub async fn run_setup(command: &str, cwd: &str, log_path: &str) -> SetupResult {
    run_setup_within(command, cwd, log_path, SETUP_TIMEOUT).await
}

async fn run_setup_within(command: &str, cwd: &str, log_path: &str, timeout: Duration) -> SetupResult {
    const NEVER_STARTED: SetupResult = SetupResult { exit_code: None };
    let Ok(log) = std::fs::File::create(log_path) else {
        return NEVER_STARTED;
    };
    let Ok(errors) = log.try_clone() else {
        return NEVER_STARTED;
    };
    let (shell, run_flag) = if cfg!(windows) { ("cmd", "/C") } else { ("sh", "-c") };
    let spawned = Command::new(shell)
        .args([run_flag, command])
        .current_dir(cwd)
        .env("CI", "1")
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(errors)
        .kill_on_drop(true)
        .spawn();
    let Ok(mut child) = spawned else {
        return NEVER_STARTED;
    };
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => SetupResult {
            exit_code: status.code(),
        },
        _ => {
            let _ = child.kill().await;
            NEVER_STARTED
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    async fn run(command: &str, timeout: Duration) -> (SetupResult, String) {
        let dir = tempfile::tempdir().expect("dir");
        let log = dir.path().join("setup.log");
        let cwd = dir.path().to_string_lossy().into_owned();
        let result = run_setup_within(command, &cwd, &log.to_string_lossy(), timeout).await;
        (result, std::fs::read_to_string(&log).unwrap_or_default())
    }

    #[tokio::test]
    async fn logs_the_output_and_reports_the_exit_code() {
        let (result, log) = run("echo \"installing $CI\"; echo oops >&2; exit 4", SETUP_TIMEOUT).await;
        assert_eq!(result, SetupResult { exit_code: Some(4) });
        assert_eq!(log, "installing 1\noops\n");
        assert!(run("true", SETUP_TIMEOUT).await.0.is_success());
    }

    #[tokio::test]
    async fn gives_up_on_an_install_that_is_stuck() {
        let (result, _) = run("sleep 30", Duration::from_millis(100)).await;
        assert_eq!(result, SetupResult { exit_code: None });
    }

    #[tokio::test]
    async fn reports_a_folder_that_does_not_exist() {
        let result = run_setup("true", "/nonexistent/atlas", "/nonexistent/atlas/setup.log").await;
        assert_eq!(result, SetupResult { exit_code: None });
    }
}
