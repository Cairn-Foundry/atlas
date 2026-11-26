// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The language Atlas speaks: French or English, the same for everything a process writes for the
//! master or for an agent.
//!
//! A text is written once per language, side by side, with [`tr!`]; the language in service picks
//! one when the text is built. It is a setting of the whole process, since a text is built far from
//! whoever chose the language: a refusal deep in the daemon, a notification, a prompt.

use std::cell::Cell;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Fr,
    En,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::Fr, Self::En];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fr => "fr",
            Self::En => "en",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|language| language.as_str() == value)
    }

    /// The language of a locale such as `fr-FR` or `fr_CA.UTF-8`: French for a French one, English
    /// for every other.
    pub fn from_locale(locale: &str) -> Self {
        let primary = locale
            .split(['-', '_', '.', '@'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if primary == "fr" { Self::Fr } else { Self::En }
    }

    /// The language of the system, used until the master picks one.
    pub fn system() -> Self {
        sys_locale::get_locale().map_or(Self::En, |locale| Self::from_locale(&locale))
    }

    fn from_index(index: u8) -> Self {
        if index == Self::En as u8 { Self::En } else { Self::Fr }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(Language::Fr as u8);

thread_local! {
    static SCOPED: Cell<Option<Language>> = const { Cell::new(None) };
}

/// The language in service: the one of [`scoped`] on this thread, else the one of the process.
pub fn current() -> Language {
    SCOPED
        .get()
        .unwrap_or_else(|| Language::from_index(CURRENT.load(Ordering::Relaxed)))
}

/// Changes the language of the whole process: every text built from now on is in it.
pub fn set_current(language: Language) {
    CURRENT.store(language as u8, Ordering::Relaxed);
}

/// Puts the previous language of the thread back when dropped.
#[must_use = "the language lasts as long as the guard"]
pub struct ScopedLanguage(Option<Language>);

impl Drop for ScopedLanguage {
    fn drop(&mut self) {
        SCOPED.set(self.0);
    }
}

/// Makes this thread speak `language` until the guard is dropped, whatever the process speaks:
/// how a test reads a text in one language while the others run in theirs.
pub fn scoped(language: Language) -> ScopedLanguage {
    ScopedLanguage(SCOPED.replace(Some(language)))
}

/// A text in the language in service, built like `format!`: the French wording, the English one,
/// then the arguments both share.
///
/// ```
/// let count = 2;
/// let text = atlas_i18n::tr!("{count} agents au travail", "{count} agents at work");
/// assert_eq!(text, "2 agents au travail");
/// ```
#[macro_export]
macro_rules! tr {
    ($french:literal, $english:literal $(, $($argument:tt)*)?) => {
        match $crate::current() {
            $crate::Language::Fr => ::std::format!($french $(, $($argument)*)?),
            $crate::Language::En => ::std::format!($english $(, $($argument)*)?),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_french_from_a_french_locale_and_english_from_any_other() {
        for locale in ["fr", "fr-FR", "fr_CA.UTF-8", "FR-be"] {
            assert_eq!(Language::from_locale(locale), Language::Fr, "{locale}");
        }
        for locale in ["en-US", "de_DE.UTF-8", "frr-DE", "C", ""] {
            assert_eq!(Language::from_locale(locale), Language::En, "{locale}");
        }
    }

    #[test]
    fn names_each_language_by_its_code() {
        for language in Language::ALL {
            assert_eq!(Language::parse(language.as_str()), Some(language));
        }
        assert_eq!(Language::parse("de"), None);
        assert_eq!(serde_json::to_string(&Language::En).expect("json"), "\"en\"");
    }

    #[test]
    fn picks_the_wording_of_the_language_in_service_with_shared_arguments() {
        let (agent, minutes) = ("SHOP-1", 4);
        let text = || tr!("{agent} attend depuis {} min", "{agent} has waited for {} min", minutes);
        assert_eq!(text(), "SHOP-1 attend depuis 4 min");
        {
            let _english = scoped(Language::En);
            assert_eq!(text(), "SHOP-1 has waited for 4 min");
            assert_eq!(tr!("bonjour", "hello"), "hello");
        }
        // The guard gone, the thread speaks the language of the process again.
        assert_eq!(text(), "SHOP-1 attend depuis 4 min");
    }
}
