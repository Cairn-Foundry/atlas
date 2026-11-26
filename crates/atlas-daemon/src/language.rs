// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The language of the factory: the one the master picked, kept in the registry, and the system's
//! until he picks one.
//!
//! What Atlas writes for the master (cards, refusals, notifications, errors) is in that language,
//! with `tr!`. What only a model reads (prompts, briefs, tool results, the catalogue) is written
//! once, in English: the model is told which language to speak to the master instead.

pub use atlas_i18n::{Language, current, tr};

use crate::error::Result;
use crate::store::Store;

const LANGUAGE_KEY: &str = "language";

/// The language the master picked, none as long as he follows the system's.
pub fn picked(store: &Store) -> Result<Option<Language>> {
    Ok(store.get_value(LANGUAGE_KEY)?.and_then(|value| Language::parse(&value)))
}

pub fn remember(store: &Store, language: Language) -> Result<()> {
    store.set_value(LANGUAGE_KEY, language.as_str())
}

/// The language in service by its English name, for a model told which language to speak.
pub fn name() -> &'static str {
    match current() {
        Language::Fr => "French",
        Language::En => "English",
    }
}

/// Makes the daemon speak `language` from now on.
#[cfg(not(test))]
pub fn speak(language: Language) {
    atlas_i18n::set_current(language);
}

/// Under test only the thread of the test changes language: the others run at the same time and
/// read their texts in theirs.
#[cfg(test)]
pub fn speak(language: Language) {
    std::mem::forget(atlas_i18n::scoped(language));
}
