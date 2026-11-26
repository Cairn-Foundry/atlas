// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The terminal of an agent session, as the daemon sees it: something to launch, type into and listen to.

use foundry_pty::{PtyEvent, PtyProcess, PtySize};

pub use foundry_pty::SpawnRequest;

/// Size of a session's terminal until the page attaches and gives its own.
pub const INITIAL_SIZE: PtySize = PtySize { cols: 120, rows: 36 };

/// Largest terminal a client may ask for: its screen is mirrored in memory, cell by cell.
pub const MAX_SIZE: PtySize = PtySize { cols: 1000, rows: 500 };

/// A running Claude Code process attached to a pseudo-terminal.
pub trait TerminalProcess: Send + Sync {
    fn write(&self, data: &[u8]);
    fn resize(&self, size: PtySize);
    fn kill(&self);
}

/// Called from background threads with what a session prints, then once with its exit.
pub type TerminalSink = Box<dyn Fn(PtyEvent) + Send + Sync>;

pub trait TerminalLauncher: Send + Sync {
    /// Starts `request`; the refusal is worded for whoever asked for the session.
    fn spawn(&self, request: &SpawnRequest, sink: TerminalSink) -> Result<Box<dyn TerminalProcess>, String>;
}

impl TerminalProcess for PtyProcess {
    fn write(&self, data: &[u8]) {
        PtyProcess::write(self, data);
    }

    fn resize(&self, size: PtySize) {
        PtyProcess::resize(self, size);
    }

    fn kill(&self) {
        PtyProcess::kill(self);
    }
}

pub struct PtyLauncher;

impl TerminalLauncher for PtyLauncher {
    fn spawn(&self, request: &SpawnRequest, sink: TerminalSink) -> Result<Box<dyn TerminalProcess>, String> {
        foundry_pty::spawn(request, sink)
            .map(|process| Box::new(process) as Box<dyn TerminalProcess>)
            .map_err(|error| error.to_string())
    }
}

/// Turns the bytes a terminal prints into text, a chunk at a time: a character cut between two
/// chunks waits for its end, and bytes that are no text become the replacement character.
#[derive(Debug, Default)]
pub struct Utf8Chunks {
    pending: Vec<u8>,
}

impl Utf8Chunks {
    pub fn push(&mut self, data: &[u8]) -> String {
        self.pending.extend_from_slice(data);
        let mut text = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(valid) => {
                    text.push_str(valid);
                    self.pending.clear();
                    return text;
                }
                Err(error) => {
                    let valid_length = error.valid_up_to();
                    text.push_str(&String::from_utf8_lossy(&self.pending[..valid_length]));
                    match error.error_len() {
                        // The end of the chunk is the start of a character: it waits for the next chunk.
                        None => {
                            self.pending.drain(..valid_length);
                            return text;
                        }
                        Some(invalid_length) => {
                            text.push(char::REPLACEMENT_CHARACTER);
                            self.pending.drain(..valid_length + invalid_length);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_the_end_of_a_character_cut_between_two_chunks() {
        let mut chunks = Utf8Chunks::default();
        let bytes = "été ✓".as_bytes();
        assert_eq!(chunks.push(&bytes[..1]), "");
        assert_eq!(chunks.push(&bytes[1..7]), "été ");
        assert_eq!(chunks.push(&bytes[7..]), "✓");
    }

    #[test]
    fn replaces_bytes_that_are_no_text_and_carries_on() {
        let mut chunks = Utf8Chunks::default();
        assert_eq!(chunks.push(b"a\xffb"), "a\u{fffd}b");
        assert_eq!(chunks.push(b"c"), "c");
    }
}
