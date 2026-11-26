// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! System prompt of the orchestrator. The catalogue section is generated from the bricks so the
//! prompt cannot drift from the yml files; it stays small on purpose, this text is paid on every run.

use crate::brick::{Brick, BrickApproval};
use crate::language;
use crate::text::collapse_whitespace;

const ROLE: &str = include_str!("../assets/orchestrator-role.txt");
/// Composing, launching, deciding, the memory of projects and following up.
const RULES: &str = include_str!("../assets/orchestrator-rules.txt");

/// One line per brick: what the orchestrator needs to choose it, never its instructions.
pub fn describe_brick(brick: &Brick) -> String {
    let mut facts = vec![brick.model.clone()];
    if !brick.requires.is_empty() {
        facts.push(format!("after {}", brick.requires.join(" or ")));
    }
    match brick.approval {
        BrickApproval::None => {}
        BrickApproval::Master => facts.push("validated by the master".to_owned()),
        BrickApproval::Orchestrator => facts.push("validated by you".to_owned()),
    }
    format!(
        "- {} ({}): {}",
        brick.id,
        facts.join(", "),
        collapse_whitespace(&brick.description)
    )
}

/// The prompt is in English whatever the master speaks: this line is what makes the orchestrator
/// answer him in his language.
fn language_rule() -> String {
    let language = language::name();
    format!(
        "The master reads {language}. Everything he reads is written in {language}: your answers, each reason, each notify_master. What you write for an agent (note, send_to_agent) keeps the words of the master."
    )
}

pub fn build_orchestrator_prompt<'a>(bricks: impl IntoIterator<Item = &'a Brick>) -> String {
    let mut catalogue = vec!["The catalogue of bricks:".to_owned()];
    catalogue.extend(
        bricks
            .into_iter()
            .filter(|brick| !brick.is_disabled)
            .map(describe_brick),
    );
    [
        ROLE.trim_end(),
        &catalogue.join("\n"),
        RULES.trim_end(),
        &language_rule(),
    ]
    .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::tests::shipped_catalogue;
    use crate::fixtures::assert_fixture;
    use crate::language::Language;

    #[test]
    fn lists_the_shipped_catalogue_between_the_role_and_the_rules() {
        assert_fixture(
            &build_orchestrator_prompt(shipped_catalogue().values()),
            "orchestrator-prompt.txt",
        );
    }

    #[test]
    fn tells_the_orchestrator_to_speak_the_language_of_the_master_whatever_the_prompt_is_written_in() {
        let catalogue = shipped_catalogue();
        assert!(build_orchestrator_prompt(catalogue.values()).ends_with(&language_rule()));
        assert!(language_rule().starts_with("The master reads French."));
        let _english = atlas_i18n::scoped(Language::En);
        assert!(language_rule().starts_with("The master reads English."));
    }

    #[test]
    fn leaves_a_disabled_brick_out_of_the_catalogue() {
        let mut catalogue = shipped_catalogue();
        catalogue["rapport"].is_disabled = true;
        let prompt = build_orchestrator_prompt(catalogue.values());
        assert!(!prompt.contains("- rapport (") && prompt.contains("- tache ("));
    }
}
