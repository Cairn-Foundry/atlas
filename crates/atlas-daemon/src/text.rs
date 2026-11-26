// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Small text shapers: branch names, folder names, command-line prompts.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

const MAX_CAMEL_WORDS: usize = 5;
const MAX_SLUG_CHARS: usize = 40;
pub const COMMAND_LINE_TEXT_CHARS: usize = 300;

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect())
        .unwrap_or_default()
}

/// A short camelCase name out of free words: `Rename SHOP account` gives `renameShopAccount`.
pub fn to_camel_case(value: &str) -> String {
    let unaccented: String = value.nfd().filter(|character| !is_combining_mark(*character)).collect();
    let words: Vec<&str> = unaccented
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(MAX_CAMEL_WORDS)
        .collect();
    match words.as_slice() {
        [] => "work".to_owned(),
        // A single word is kept as written, apart from its first letter: it may be camelCase already.
        [only] => {
            let mut chars = only.chars();
            chars
                .next()
                .map(|first| first.to_lowercase().chain(chars).collect())
                .unwrap_or_default()
        }
        [first, rest @ ..] => first.to_lowercase() + &rest.iter().map(|word| capitalized(word)).collect::<String>(),
    }
}

/// A lowercase, dash-separated piece of a folder name.
pub fn slugify(value: &str) -> String {
    let mut slug = String::new();
    for character in value.to_lowercase().chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            slug.push(character);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_end_matches('-').chars().take(MAX_SLUG_CHARS).collect();
    if slug.is_empty() { "ticket".to_owned() } else { slug }
}

/// Runs of whitespace reduced to one space, the ends trimmed.
pub fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Makes free text safe as part of a command-line prompt: one line, no double quote (Windows).
pub fn to_command_line_text(value: &str) -> String {
    collapse_whitespace(value)
        .replace('"', "'")
        .chars()
        .take(COMMAND_LINE_TEXT_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_camel_case_name_from_free_words() {
        assert_eq!(to_camel_case("Rename SHOP account"), "renameShopAccount");
        assert_eq!(to_camel_case("Déjà réglé à l'été"), "dejaRegleALEte");
        assert_eq!(to_camel_case("one two three four five six"), "oneTwoThreeFourFive");
    }

    #[test]
    fn keeps_a_single_word_as_written_and_names_nothing_work() {
        assert_eq!(to_camel_case("RenameShopAccount"), "renameShopAccount");
        assert_eq!(to_camel_case("--"), "work");
    }

    #[test]
    fn slugifies_a_folder_name() {
        assert_eq!(slugify("SHOP-123 / Été"), "shop-123-t");
        assert_eq!(slugify("***"), "ticket");
        assert_eq!(slugify(&"a".repeat(60)).len(), MAX_SLUG_CHARS);
    }

    #[test]
    fn makes_text_fit_for_a_command_line() {
        assert_eq!(to_command_line_text("  a \"quoted\"\n line "), "a 'quoted' line");
        assert_eq!(
            to_command_line_text(&"é".repeat(400)).chars().count(),
            COMMAND_LINE_TEXT_CHARS
        );
    }
}
