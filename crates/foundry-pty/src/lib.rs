// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Pseudo-terminal sessions: a command running on a PTY, and a mirror of what its screen shows.

mod command;
mod mirror;
mod process;

pub use command::{ResolvedCommand, resolve_command, resolve_windows_command};
pub use mirror::TerminalMirror;
pub use process::{PtyError, PtyEvent, PtyProcess, PtySize, SpawnRequest, spawn};
