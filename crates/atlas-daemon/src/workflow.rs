// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The rules of a workflow: which bricks may follow which, how review passes loop, when a session is shared.

use crate::language::tr;
use chrono::{DateTime, Utc};

use crate::brick::{Brick, Severity};
use crate::catalogue::Catalogue;
use crate::clock::elapsed_ms;
use crate::contract::StepState;
use crate::error::{Error, Result};
use crate::store::AgentRecord;
use crate::text::collapse_whitespace;

const MS_PER_MINUTE: i64 = 60_000;

pub fn require_brick<'a>(catalogue: &'a Catalogue, brick_id: &str) -> Result<&'a Brick> {
    catalogue.get(brick_id).ok_or_else(|| {
        Error::internal(tr!(
            "brique inconnue dans le catalogue : {brick_id}",
            "unknown brick in the catalogue: {brick_id}"
        ))
    })
}

/// Why `brick_ids` cannot be the steps of an agent, empty when it can. `before` holds the bricks
/// the agent already went through: they count for the dependencies of the new steps.
pub fn workflow_issues(catalogue: &Catalogue, brick_ids: &[String], before: &[String]) -> Vec<String> {
    let mut issues = Vec::new();
    let mut seen: Vec<&str> = before.iter().map(String::as_str).collect();
    for brick_id in brick_ids {
        let Some(brick) = catalogue.get(brick_id) else {
            let known: Vec<&str> = catalogue.keys().map(String::as_str).collect();
            issues.push(tr!(
                "brique inconnue : {brick_id}. Briques du catalogue : {}",
                "unknown brick: {brick_id}. Bricks of the catalogue: {}",
                known.join(", ")
            ));
            continue;
        };
        if brick.is_disabled {
            issues.push(tr!("brique désactivée : {brick_id}", "disabled brick: {brick_id}"));
        }
        if !brick.requires.is_empty() && !brick.requires.iter().any(|required| seen.contains(&required.as_str())) {
            issues.push(tr!(
                "{brick_id} doit venir après {}",
                "{brick_id} must come after {}",
                brick.requires.join(&tr!(" ou ", " or "))
            ));
        }
        seen.push(brick_id);
    }
    issues
}

/// What makes a brick a review whose findings call for a correction and another pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopingReview<'a> {
    pub fail_on: &'a [Severity],
    pub max_passes: u32,
    pub fixed_by: &'a str,
}

pub fn looping_review(brick: &Brick) -> Option<LoopingReview<'_>> {
    Some(LoopingReview {
        fail_on: brick.fail_on.as_deref()?,
        max_passes: brick.max_passes?,
        fixed_by: brick.fixed_by.as_deref()?,
    })
}

/// Model of a review pass: the first reads the whole work, the later ones only the fix commits.
pub fn pass_model(brick: &Brick, pass: u32) -> &str {
    match &brick.later_pass_model {
        Some(model) if pass > 1 => model,
        _ => &brick.model,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewOutcome {
    /// Nothing at the threshold: the correction planned after the review, if any, is dropped.
    Converged,
    /// Findings nobody asked to fix: the review was the point, the agent moves on.
    ReportOnly,
    /// Findings to fix, then a fresh pass on the fixes.
    FixAndReview,
    /// Findings left after the last pass: the master decides.
    Exhausted,
}

/// What a finished review pass leads to. `is_fix_expected` is true when the workflow shows the
/// findings must be closed: a correction follows, the pass was born from one, or a later step
/// depends on the review (a ship never leaves with open findings).
pub fn evaluate_review_pass(
    review: LoopingReview<'_>,
    pass: u32,
    findings: u32,
    is_fix_expected: bool,
) -> ReviewOutcome {
    if findings == 0 {
        ReviewOutcome::Converged
    } else if !is_fix_expected {
        ReviewOutcome::ReportOnly
    } else if pass >= review.max_passes {
        ReviewOutcome::Exhausted
    } else {
        ReviewOutcome::FixAndReview
    }
}

/// What a later step needs to know of an open session to carry on in it.
#[derive(Debug, Clone, Copy)]
pub struct SessionTraits<'a> {
    pub group: &'a str,
    pub model: &'a str,
    pub has_subagents: bool,
    pub skills: &'a [String],
    pub subagent_types: &'a [String],
}

/// Whether the session of one step can carry on with the next: same group and model, and the next
/// brick neither starts from a slash command, nor wants fresh eyes, nor a session of its own, nor
/// subagents, skills or subagent types the session was opened without: a session gets them at start.
pub fn can_share_session(current: SessionTraits<'_>, next: &Brick, next_model: &str) -> bool {
    current.group == next.session
        && current.model == next_model
        && next.command.is_none()
        && next.max_passes.is_none()
        && !next.fresh_session
        && (current.has_subagents || !next.subagents)
        && next.skills.iter().all(|skill| current.skills.contains(skill))
        && next
            .subagent_types
            .iter()
            .all(|subagent| current.subagent_types.contains(subagent))
}

/// Built-in Claude Code tools an agent never needs: their definitions alone weigh about 18k tokens,
/// reread on every turn. AskUserQuestion goes too: a question asked with it leaves the agent shown
/// as working while it waits, where ask_user tells Atlas and the master.
const UNUSED_AGENT_TOOLS: [&str; 7] = [
    "Artifact",
    "Workflow",
    "ScheduleWakeup",
    "ShareOnboardingGuide",
    "ReportFindings",
    "ListAgents",
    "AskUserQuestion",
];
const SUBAGENT_TOOL: &str = "Agent";

/// Tools withheld from a session opened for `brick`: the unused ones, and Agent when the brick may not launch subagents.
pub fn withheld_tools(brick: &Brick) -> Vec<&'static str> {
    let mut tools = UNUSED_AGENT_TOOLS.to_vec();
    if !brick.subagents {
        tools.push(SUBAGENT_TOOL);
    }
    tools
}

/// The `{name}` placeholders of a command, each with the byte range it covers.
fn placeholders(template: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut found = Vec::new();
    let mut rest = 0;
    while let Some(open) = template[rest..].find('{').map(|offset| rest + offset) {
        let name_length = template[open + 1..].bytes().take_while(u8::is_ascii_alphabetic).count();
        let close = open + 1 + name_length;
        if name_length > 0 && template[close..].starts_with('}') {
            found.push((open..close + 1, &template[open + 1..close]));
            rest = close + 1;
        } else {
            rest = open + 1;
        }
    }
    found
}

/// Fills the `{name}` placeholders of a brick command; an unknown name is left as it is.
pub fn render_command(template: &str, values: &[(&str, &str)]) -> String {
    let mut rendered = String::new();
    let mut copied = 0;
    for (range, name) in placeholders(template) {
        rendered.push_str(&template[copied..range.start]);
        match values.iter().find(|(key, _)| *key == name) {
            Some((_, value)) => rendered.push_str(value),
            None => rendered.push_str(&template[range.clone()]),
        }
        copied = range.end;
    }
    rendered.push_str(&template[copied..]);
    collapse_whitespace(&rendered)
}

/// Names of the placeholders a command uses.
pub fn command_placeholders(template: &str) -> Vec<&str> {
    placeholders(template).into_iter().map(|(_, name)| name).collect()
}

/// Milliseconds the agent has actively spent in its step: waiting for the master does not count.
pub fn active_step_ms(agent: &AgentRecord, now: DateTime<Utc>) -> i64 {
    let total = crate::clock::parse_iso(&agent.step_started_at).map_or(0, |start| (now - start).num_milliseconds());
    let current_wait = agent.waiting_since.as_deref().map_or(0, |since| elapsed_ms(since, now));
    (total - agent.step_wait_ms - current_wait).max(0)
}

pub fn is_over_threshold(brick: &Brick, agent: &AgentRecord, now: DateTime<Utc>) -> bool {
    active_step_ms(agent, now) > i64::from(brick.threshold_minutes) * MS_PER_MINUTE
}

/// The agent's workflow on one line, its current step in brackets.
pub fn describe_path(agent: &AgentRecord) -> String {
    agent
        .steps
        .iter()
        .filter_map(|step| match step.state {
            StepState::Skipped => None,
            StepState::Current => Some(format!("[{}]", step.name)),
            StepState::Done => Some(format!("{} (done)", step.name)),
            StepState::External => Some(format!("{} (done outside Atlas)", step.name)),
            StepState::Upcoming => Some(step.name.clone()),
        })
        .collect::<Vec<_>>()
        .join(" > ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::tests::shipped_catalogue;
    use crate::testing::ids;

    #[test]
    fn accepts_the_default_workflow_and_names_what_breaks_another() {
        let catalogue = shipped_catalogue();
        let default_steps = ids(&[
            "qualification",
            "plan",
            "implementation",
            "tests",
            "review",
            "correction",
            "ship",
        ]);
        assert!(workflow_issues(&catalogue, &default_steps, &[]).is_empty());
        let issues = workflow_issues(&catalogue, &ids(&["correction", "ghost"]), &[]);
        assert_eq!(
            issues[0],
            format!(
                "correction doit venir après {}",
                catalogue["correction"].requires.join(" ou ")
            )
        );
        assert!(issues[1].starts_with("brique inconnue : ghost. Briques du catalogue : correction, "));
    }

    #[test]
    fn counts_the_bricks_already_done_for_the_dependencies() {
        let catalogue = shipped_catalogue();
        assert!(workflow_issues(&catalogue, &ids(&["correction"]), &catalogue["correction"].requires).is_empty());
    }

    #[test]
    fn offers_a_disabled_brick_to_no_workflow() {
        let mut catalogue = shipped_catalogue();
        catalogue["rapport"].is_disabled = true;
        assert_eq!(
            workflow_issues(&catalogue, &ids(&["rapport"]), &[]),
            ["brique désactivée : rapport"]
        );
    }

    #[test]
    fn decides_what_follows_a_review_pass() {
        let catalogue = shipped_catalogue();
        let review = looping_review(&catalogue["review"]).expect("looping review");
        assert_eq!(evaluate_review_pass(review, 1, 0, true), ReviewOutcome::Converged);
        assert_eq!(evaluate_review_pass(review, 1, 2, false), ReviewOutcome::ReportOnly);
        assert_eq!(evaluate_review_pass(review, 1, 2, true), ReviewOutcome::FixAndReview);
        assert_eq!(
            evaluate_review_pass(review, review.max_passes, 2, true),
            ReviewOutcome::Exhausted
        );
        assert!(looping_review(&catalogue["plan"]).is_none());
    }

    #[test]
    fn reads_later_passes_with_the_lighter_model() {
        let catalogue = shipped_catalogue();
        assert_eq!(pass_model(&catalogue["review"], 1), "opus");
        assert_eq!(pass_model(&catalogue["review"], 2), "sonnet");
        assert_eq!(pass_model(&catalogue["plan"], 2), catalogue["plan"].model);
    }

    #[test]
    fn shares_a_session_only_within_a_group_and_model() {
        let catalogue = shipped_catalogue();
        let tests = &catalogue["tests"];
        let session = SessionTraits {
            group: &tests.session,
            model: &tests.model,
            has_subagents: false,
            skills: &[],
            subagent_types: &[],
        };
        assert_eq!(
            can_share_session(session, tests, &tests.model),
            !tests.fresh_session && !tests.subagents
        );
        assert!(!can_share_session(
            SessionTraits {
                model: "other",
                ..session
            },
            tests,
            &tests.model
        ));
        // A review starts from a slash command and loops: always a session of its own.
        let review = &catalogue["review"];
        assert!(!can_share_session(
            SessionTraits {
                group: &review.session,
                model: &review.model,
                has_subagents: true,
                ..session
            },
            review,
            &review.model
        ));
        // A brick that needs a skill the session was opened without starts again with it.
        let mut skilled = tests.clone();
        skilled.skills = vec!["deep-review".to_owned()];
        assert!(!can_share_session(session, &skilled, &tests.model));
        let skills = ["deep-review".to_owned(), "other".to_owned()];
        assert!(can_share_session(
            SessionTraits {
                skills: &skills,
                ..session
            },
            &skilled,
            &tests.model
        ));
    }

    #[test]
    fn withholds_the_agent_tool_unless_the_brick_may_launch_subagents() {
        let catalogue = shipped_catalogue();
        assert!(!withheld_tools(&catalogue["review"]).contains(&"Agent"));
        assert!(withheld_tools(&catalogue["plan"]).contains(&"Agent"));
    }

    #[test]
    fn fills_the_placeholders_of_a_command_and_leaves_unknown_ones() {
        let template = "/review {range}\n  du dépôt {worktree}. {unknown} {} {a1}";
        assert_eq!(
            render_command(template, &[("range", "la branche x"), ("worktree", "/wt")]),
            "/review la branche x du dépôt /wt. {unknown} {} {a1}"
        );
        assert_eq!(command_placeholders(template), ["range", "worktree", "unknown"]);
    }
}
