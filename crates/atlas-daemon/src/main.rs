// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! `atlas-daemon`: runs the factory, or answers a Claude Code hook with `atlas-daemon hook <event>`.

use std::io::{Read, Write};
use std::process::ExitCode;

use atlas_daemon::contract::DAEMON_VERSION;
use atlas_daemon::{daemon, hook};
use atlas_i18n::{Language, tr};

/// Never fails the agent: whatever happens, the hook exits 0 and prints at most the daemon's answer.
fn answer_hook(event: &str) -> ExitCode {
    let mut payload = String::new();
    let _ = std::io::stdin().read_to_string(&mut payload);
    if let Some(output) = hook::run_hook(event, |name| std::env::var(name).ok(), &payload) {
        let _ = std::io::stdout().write_all(output.as_bytes());
    }
    ExitCode::SUCCESS
}

fn run_daemon() -> ExitCode {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
    // One thread runs the factory: between two awaits nothing else touches an agent.
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("[atlas] {error}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(daemon::run(atlas_link::atlas_home())) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("[atlas] {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    // The language of the system until the daemon reads the one the master picked.
    atlas_i18n::set_current(Language::system());
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => run_daemon(),
        ["hook", event] => answer_hook(event),
        ["--version"] => {
            println!("atlas-daemon {DAEMON_VERSION}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "{}",
                tr!(
                    "Usage : atlas-daemon [hook <événement> | --version]",
                    "Usage: atlas-daemon [hook <event> | --version]"
                )
            );
            ExitCode::FAILURE
        }
    }
}
