// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The language of the factory, changed while it runs.

use super::{AgentService, Event};
use crate::error::Result;
use crate::language::{self, Language};

impl AgentService {
    /// Makes the whole factory speak `language`: what Atlas writes for the master from now on, and
    /// what the orchestrator and the agents answer him. What was already written stays as it is.
    pub fn set_language(&self, language: Language) -> Result<()> {
        language::remember(self.store(), language)?;
        language::speak(language);
        self.emit(Event::Language);
        Ok(())
    }
}
