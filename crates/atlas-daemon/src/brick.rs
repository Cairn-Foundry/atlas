// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A brick: one file of the catalogue, a step the orchestrator can put in an agent's workflow.
//!
//! The JSON Schema the daemon writes next to the bricks, for editors and CI, is derived from the
//! type below; the rules of `validated` are the ones the daemon enforces.

use crate::language::tr;
use std::collections::HashSet;

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::contract::{Issue, is_brick_id};

/// Section of the agent's file that is not written by a brick: the master's note at launch.
pub const NOTE_SECTION: &str = "note";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BrickApproval {
    #[default]
    None,
    Master,
    Orchestrator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

/// Strict, so a misspelled key is refused instead of silently ignored.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Brick {
    #[schemars(regex(pattern = "^[a-z][a-z0-9-]*$"))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub name: String,
    /// What the brick does and when to use it: all the orchestrator reads to assemble a workflow.
    #[schemars(length(min = 1))]
    pub description: String,
    #[schemars(length(min = 1))]
    pub model: String,
    /// Session group: consecutive bricks of the same group and model share one Claude Code session.
    #[schemars(regex(pattern = "^[a-z][a-z0-9-]*$"))]
    pub session: String,
    #[schemars(range(min = 1))]
    pub threshold_minutes: u32,
    /// Bricks of which at least one must come earlier in the agent's workflow; empty for none.
    #[serde(default)]
    #[schemars(inner(regex(pattern = "^[a-z][a-z0-9-]*$")))]
    pub requires: Vec<String>,
    /// Sections of the agent's file handed to the brick: brick ids, or `note` for the master's note.
    #[serde(default)]
    #[schemars(inner(regex(pattern = "^[a-z][a-z0-9-]*$")))]
    pub reads: Vec<String>,
    /// Who validates the end of the step.
    #[serde(default)]
    pub approval: BrickApproval,
    /// Whether the brick may launch subagents.
    #[serde(default)]
    pub subagents: bool,
    /// Skills of `~/.atlas/skills` the brick's session is given, on top of those of Claude Code.
    #[serde(default)]
    #[schemars(inner(regex(pattern = "^[a-z][a-z0-9-]*$")))]
    pub skills: Vec<String>,
    /// Subagents of `~/.atlas/subagents` the brick's session is given; asks for `subagents`.
    #[serde(default)]
    #[schemars(inner(regex(pattern = "^[a-z][a-z0-9-]*$")))]
    pub subagent_types: Vec<String>,
    /// Whether the step may only close with everything committed: later steps read commits, not a dirty tree.
    #[serde(default)]
    pub requires_clean_tree: bool,
    /// Fields `complete_step` requires, each with what it must hold: the brick's section of the file.
    #[schemars(length(min = 1), inner(length(min = 1)), extend("propertyNames" = {"pattern": "^[a-z][A-Za-z0-9]*$"}))]
    pub writes: IndexMap<String, String>,
    #[schemars(length(min = 1))]
    pub instructions: String,
    /// Whether each step of the brick opens a session of its own, never carrying on in one of its group.
    #[serde(default)]
    pub fresh_session: bool,
    /// Whether the step builds or runs the code: it waits for the dependencies Atlas installs in a new worktree.
    #[serde(default)]
    pub needs_dependencies: bool,
    /// Slash command the session starts with, for a brick driven by a user-invoked skill.
    #[serde(default)]
    #[schemars(length(min = 1))]
    pub command: Option<String>,
    /// Review bricks: passes allowed before the master is asked.
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub max_passes: Option<u32>,
    /// Review bricks: severities that call for a correction and another pass.
    #[serde(default)]
    #[schemars(length(min = 1))]
    pub fail_on: Option<Vec<Severity>>,
    /// Review bricks: model of the passes after the first, which reread only the fix commits.
    #[serde(default)]
    #[schemars(length(min = 1))]
    pub later_pass_model: Option<String>,
    /// Review bricks: the brick that fixes the findings; the daemon alternates it with new passes.
    #[serde(default)]
    #[schemars(regex(pattern = "^[a-z][a-z0-9-]*$"))]
    pub fixed_by: Option<String>,
    /// Set from the list of disabled bricks, never from the brick's file: a disabled brick stays
    /// known to the agents that already have it, and is offered to no new workflow.
    #[serde(skip)]
    #[schemars(skip)]
    pub is_disabled: bool,
}

fn is_field_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|character| character.is_ascii_alphanumeric())
}

fn id_rule() -> String {
    tr!(
        "lettres minuscules, chiffres et tirets",
        "lowercase letters, digits and dashes"
    )
}

impl Brick {
    /// The brick with its free texts trimmed, or what is wrong with it.
    pub fn validated(mut self) -> Result<Self, Vec<Issue>> {
        self.description = self.description.trim().to_owned();
        self.instructions = self.instructions.trim().to_owned();
        self.command = self.command.map(|command| command.trim().to_owned());

        let mut issues = Vec::new();
        let mut require = |is_valid: bool, path: &str, message: &str| {
            if !is_valid {
                issues.push(Issue::new(path, message));
            }
        };
        require(is_brick_id(&self.id), "id", &id_rule());
        require(!self.name.is_empty(), "name", &tr!("texte vide", "empty text"));
        require(
            !self.description.is_empty(),
            "description",
            &tr!("texte vide", "empty text"),
        );
        require(!self.model.is_empty(), "model", &tr!("texte vide", "empty text"));
        require(is_brick_id(&self.session), "session", &id_rule());
        require(
            self.threshold_minutes > 0,
            "thresholdMinutes",
            &tr!("entier positif attendu", "positive integer expected"),
        );
        require(self.requires.iter().all(|id| is_brick_id(id)), "requires", &id_rule());
        require(self.reads.iter().all(|id| is_brick_id(id)), "reads", &id_rule());
        require(self.skills.iter().all(|name| is_brick_id(name)), "skills", &id_rule());
        require(
            self.subagent_types.iter().all(|name| is_brick_id(name)),
            "subagentTypes",
            &id_rule(),
        );
        require(
            self.subagent_types.is_empty() || self.subagents,
            "subagentTypes",
            &tr!("subagentTypes demande subagents", "subagentTypes requires subagents"),
        );
        require(
            !self.writes.is_empty(),
            "writes",
            &tr!("au moins un champ", "at least one field"),
        );
        require(
            self.writes.keys().all(|field| is_field_name(field)),
            "writes",
            &tr!("nom de champ en camelCase", "field name in camelCase"),
        );
        require(
            self.writes.values().all(|expected| !expected.is_empty()),
            "writes",
            &tr!("texte vide", "empty text"),
        );
        require(
            !self.instructions.is_empty(),
            "instructions",
            &tr!("texte vide", "empty text"),
        );
        require(
            self.command.as_deref() != Some(""),
            "command",
            &tr!("texte vide", "empty text"),
        );
        require(
            self.max_passes != Some(0),
            "maxPasses",
            &tr!("entier positif attendu", "positive integer expected"),
        );
        require(
            self.fail_on.as_ref().is_none_or(|severities| !severities.is_empty()),
            "failOn",
            &tr!("au moins une gravité", "at least one severity"),
        );
        require(
            self.later_pass_model.as_deref() != Some(""),
            "laterPassModel",
            &tr!("texte vide", "empty text"),
        );
        require(self.fixed_by.as_deref().is_none_or(is_brick_id), "fixedBy", &id_rule());
        require(
            !self.requires.contains(&self.id),
            "requires",
            &tr!(
                "une brique ne peut pas s'exiger elle-même",
                "a brick cannot require itself"
            ),
        );
        require(
            self.fail_on.is_none() || self.max_passes.is_some(),
            "maxPasses",
            &tr!("failOn demande maxPasses", "failOn requires maxPasses"),
        );
        require(
            self.later_pass_model.is_none() || self.max_passes.is_some(),
            "laterPassModel",
            &tr!("laterPassModel demande maxPasses", "laterPassModel requires maxPasses"),
        );
        require(
            self.fixed_by.is_none() || self.fail_on.is_some(),
            "failOn",
            &tr!("fixedBy demande failOn", "fixedBy requires failOn"),
        );
        if issues.is_empty() { Ok(self) } else { Err(issues) }
    }
}

/// Problems that only show with the whole catalogue in hand: duplicate ids, references to unknown
/// bricks, and bricks in service that depend on disabled ones.
pub fn catalogue_issues(bricks: &[Brick]) -> Vec<String> {
    let mut issues = Vec::new();
    let mut ids = HashSet::new();
    for brick in bricks {
        if !ids.insert(brick.id.as_str()) {
            issues.push(tr!("id de brique en double : {}", "duplicate brick id: {}", brick.id));
        }
    }
    for brick in bricks {
        for required in brick
            .requires
            .iter()
            .filter(|required| !ids.contains(required.as_str()))
        {
            issues.push(tr!(
                "{} : requires cite une brique inconnue ({required})",
                "{}: requires names an unknown brick ({required})",
                brick.id
            ));
        }
        if let Some(fixed_by) = brick.fixed_by.as_deref().filter(|fixed_by| !ids.contains(fixed_by)) {
            issues.push(tr!(
                "{} : fixedBy cite une brique inconnue ({fixed_by})",
                "{}: fixedBy names an unknown brick ({fixed_by})",
                brick.id
            ));
        }
        for section in brick
            .reads
            .iter()
            .filter(|section| *section != NOTE_SECTION && !ids.contains(section.as_str()))
        {
            issues.push(tr!(
                "{} : reads cite une section inconnue ({section})",
                "{}: reads names an unknown section ({section})",
                brick.id
            ));
        }
    }
    let is_disabled = |id: &str| bricks.iter().any(|brick| brick.id == id && brick.is_disabled);
    for brick in bricks.iter().filter(|brick| !brick.is_disabled) {
        if !brick.requires.is_empty() && brick.requires.iter().all(|required| is_disabled(required)) {
            issues.push(tr!(
                "{} : toutes les briques de requires sont désactivées ({})",
                "{}: every brick of requires is disabled ({})",
                brick.id,
                brick.requires.join(", ")
            ));
        }
        if let Some(fixed_by) = brick.fixed_by.as_deref().filter(|fixed_by| is_disabled(fixed_by)) {
            issues.push(tr!(
                "{} : fixedBy cite une brique désactivée ({fixed_by})",
                "{}: fixedBy names a disabled brick ({fixed_by})",
                brick.id
            ));
        }
    }
    issues
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn brick(yaml: &str) -> Result<Brick, Vec<Issue>> {
        serde_yaml_ng::from_str::<Brick>(yaml).expect("yaml").validated()
    }

    const MINIMAL: &str = "id: plan\nname: Plan\ndescription: Un plan\nmodel: opus\nsession: build\nthresholdMinutes: 10\nwrites: { plan: Le plan }\ninstructions: Planifie\n";

    #[test]
    fn fills_the_defaults_of_a_minimal_brick() {
        let brick = brick(MINIMAL).expect("brick");
        assert_eq!(brick.approval, BrickApproval::None);
        assert!(brick.requires.is_empty() && !brick.subagents && !brick.fresh_session);
        assert!(brick.skills.is_empty() && brick.subagent_types.is_empty());
        assert_eq!(brick.command, None);
    }

    #[test]
    fn refuses_a_misspelled_key() {
        let error = serde_yaml_ng::from_str::<Brick>(&format!("{MINIMAL}treshold: 3\n")).expect_err("unknown key");
        assert!(error.to_string().contains("treshold"), "{error}");
    }

    #[test]
    fn names_each_invalid_field() {
        let yaml = MINIMAL
            .replace("id: plan", "id: Plan")
            .replace("thresholdMinutes: 10", "thresholdMinutes: 0");
        let issues = brick(&format!(
            "{yaml}requires: [Plan]\nfailOn: [high]\nfixedBy: correction\n"
        ))
        .expect_err("issues");
        let paths: Vec<&str> = issues.iter().map(|issue| issue.path.as_str()).collect();
        assert_eq!(paths, ["id", "thresholdMinutes", "requires", "requires", "maxPasses"]);
    }

    #[test]
    fn refuses_review_options_without_what_they_depend_on() {
        let issues = brick(&format!("{MINIMAL}laterPassModel: sonnet\nfixedBy: correction\n")).expect_err("issues");
        let messages: Vec<&str> = issues.iter().map(|issue| issue.message.as_str()).collect();
        assert_eq!(messages, ["laterPassModel demande maxPasses", "fixedBy demande failOn"]);
    }

    #[test]
    fn refuses_subagent_types_on_a_brick_without_subagents() {
        let issues = brick(&format!("{MINIMAL}skills: [Deep]\nsubagentTypes: [minos]\n")).expect_err("issues");
        let messages: Vec<(&str, &str)> = issues
            .iter()
            .map(|issue| (issue.path.as_str(), issue.message.as_str()))
            .collect();
        assert_eq!(
            messages,
            [
                ("skills", id_rule().as_str()),
                ("subagentTypes", "subagentTypes demande subagents")
            ]
        );
    }

    #[test]
    fn finds_duplicates_and_unknown_references_across_the_catalogue() {
        let plan = brick(MINIMAL).expect("plan");
        let review = brick(&format!(
            "{}requires: [ghost]\nreads: [note, phantom]\nmaxPasses: 2\nfailOn: [high]\nfixedBy: nowhere\n",
            MINIMAL.replace("id: plan", "id: review")
        ))
        .expect("review");
        assert_eq!(
            catalogue_issues(&[plan.clone(), plan, review]),
            [
                "id de brique en double : plan",
                "review : requires cite une brique inconnue (ghost)",
                "review : fixedBy cite une brique inconnue (nowhere)",
                "review : reads cite une section inconnue (phantom)",
            ]
        );
    }

    #[test]
    fn refuses_to_disable_what_a_brick_in_service_depends_on() {
        let mut qualification = brick(&MINIMAL.replace("id: plan", "id: qualification")).expect("qualification");
        let mut correction = brick(&MINIMAL.replace("id: plan", "id: correction")).expect("correction");
        let plan = brick(&format!("{MINIMAL}requires: [qualification]\n")).expect("plan");
        let review = brick(&format!(
            "{}maxPasses: 2\nfailOn: [high]\nfixedBy: correction\n",
            MINIMAL.replace("id: plan", "id: review")
        ))
        .expect("review");
        qualification.is_disabled = true;
        correction.is_disabled = true;
        let mut bricks = vec![qualification, correction, plan, review];
        assert_eq!(
            catalogue_issues(&bricks),
            [
                "plan : toutes les briques de requires sont désactivées (qualification)",
                "review : fixedBy cite une brique désactivée (correction)",
            ]
        );
        // Disabled too, they no longer count.
        bricks[2].is_disabled = true;
        bricks[3].is_disabled = true;
        assert!(catalogue_issues(&bricks).is_empty());
    }
}
