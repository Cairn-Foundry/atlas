// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Atlas daemon: the registry of agents, their sessions, the orchestrator, the MCP servers
//! and the HTTP API the clients (the front, Cairn, any tool) talk to.
//!
//! Only what the `atlas-daemon` binary runs is public: the rest stays private, so the compiler
//! reports whatever is no longer used.

mod brick;
mod catalogue;
mod claude;
mod clock;
mod config;
pub mod contract;
pub mod daemon;
mod error;
mod files;
pub mod hook;
mod language;
mod mcp;
mod notifier;
mod orchestrator;
mod orchestrator_prompt;
mod projects;
mod prompts;
mod server;
mod service;
mod setup;
mod store;
mod terminal;
mod text;
mod workflow;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded.rs"));
}

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod testing;
