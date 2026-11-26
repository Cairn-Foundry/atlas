// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The two ways a daemon operation fails.

use crate::language::tr;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An expected refusal, worded for whoever asked (an agent, the orchestrator, the master).
    #[error("{0}")]
    Refusal(String),
    /// Anything else is a bug or a broken environment.
    #[error("{0}")]
    Internal(String),
}

impl Error {
    pub fn refusal(message: impl Into<String>) -> Self {
        Self::Refusal(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Internal(tr!("registre : {error}", "registry: {error}"))
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Internal(tr!("JSON : {error}", "JSON: {error}"))
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Internal(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Returns early with a refusal built like `tr!`: the French wording, the English one, the arguments.
macro_rules! refuse {
    ($($argument:tt)*) => {
        return Err($crate::error::Error::Refusal($crate::language::tr!($($argument)*)))
    };
}
pub(crate) use refuse;
