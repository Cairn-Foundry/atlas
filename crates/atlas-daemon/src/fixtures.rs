// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Sample agents shared by the tests, and the pinned texts the daemon must produce for them.

use std::path::Path;

use crate::contract::{AgentStatus, RepoView, StepOutput, StepState};
use crate::store::{AgentRecord, ExtraWorkspace, StepRecord};

/// Compares a text handed to an agent or to the orchestrator with the one pinned under `name`:
/// a prompt only changes on purpose. `ATLAS_UPDATE_FIXTURES=1 cargo test` rewrites the pinned ones.
pub(crate) fn assert_fixture(actual: &str, name: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    if std::env::var_os("ATLAS_UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, actual).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    }
    let pinned = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    assert!(
        actual == pinned,
        "{} is not what the daemon writes: rerun the tests with ATLAS_UPDATE_FIXTURES=1\n{actual}",
        path.display()
    );
}

pub(crate) fn step(id: &str, brick_id: &str, name: &str, state: StepState, model: &str) -> StepRecord {
    StepRecord {
        id: id.to_owned(),
        brick_id: brick_id.to_owned(),
        name: name.to_owned(),
        state,
        pass: 1,
        model: model.to_owned(),
        started_at: None,
        ended_at: None,
        wait_ms: 0,
        usage: None,
        output: None,
        session_id: None,
        head_sha: None,
        findings: None,
    }
}

fn output(fields: &[(&str, &str)]) -> Option<StepOutput> {
    Some(
        fields
            .iter()
            .map(|(field, value)| ((*field).to_owned(), (*value).to_owned()))
            .collect(),
    )
}

/// An agent on the second pass of its review, with a second repository and a merge request.
pub(crate) fn review_agent() -> AgentRecord {
    let done =
        |id: &str, brick_id: &str, name: &str, model: &str, ended_at: &str, fields: &[(&str, &str)]| StepRecord {
            ended_at: Some(ended_at.to_owned()),
            output: output(fields),
            ..step(id, brick_id, name, StepState::Done, model)
        };
    AgentRecord {
        id: "agent-1".to_owned(),
        ticket_ref: "SHOP-12".to_owned(),
        title: "Renommer le \"compte\" boutique".to_owned(),
        repo_name: "shop".to_owned(),
        repo_path: "/repos/shop".to_owned(),
        worktree_path: "/wt/shop-12".to_owned(),
        branch: "fix/SHOP-12/renameShopAccount".to_owned(),
        base_branch: "main".to_owned(),
        step_id: "review".to_owned(),
        steps: vec![
            done(
                "s1",
                "qualification",
                "Qualification",
                "sonnet",
                "2026-01-01T10:05:00.000Z",
                &[("need", "Renommer le compte\nboutique"), ("size", "petit")],
            ),
            step("s2", "plan", "Plan", StepState::External, "opus"),
            step("s3", "implementation", "Implémentation", StepState::Skipped, "sonnet"),
            StepRecord {
                head_sha: Some("abc123".to_owned()),
                ..done(
                    "s4",
                    "review",
                    "Review",
                    "opus",
                    "2026-01-01T11:00:00.000Z",
                    &[
                        ("findings", "un finding"),
                        ("criteria", "couverts"),
                        ("dropped", "aucun"),
                    ],
                )
            },
            done(
                "s5",
                "correction",
                "Correction",
                "sonnet",
                "2026-01-01T11:30:00.000Z",
                &[("fixed", "corrigé")],
            ),
            StepRecord {
                pass: 2,
                ..step("s6", "review", "Review", StepState::Current, "sonnet")
            },
            step("s7", "ship", "Ship", StepState::Upcoming, "sonnet"),
        ],
        workspaces: vec![ExtraWorkspace {
            repo_name: "web".to_owned(),
            repo_path: "/repos/web".to_owned(),
            worktree_path: "/wt/shop-12-web".to_owned(),
            branch: "fix/SHOP-12/renameShopAccount".to_owned(),
            base_branch: "develop".to_owned(),
        }],
        status: AgentStatus::Working,
        status_reason: None,
        step_started_at: "2026-01-01T11:30:00.000Z".to_owned(),
        step_wait_ms: 0,
        waiting_since: None,
        mr_url: Some("https://forge.test/mr/4".to_owned()),
        live_session_id: None,
        approval_requested: false,
        threshold_alerted: false,
        note: Some("Garder l'ancien nom en alias.".to_owned()),
        stop_blocks: 0,
        owns_worktree: true,
        archived_at: None,
        created_at: "2026-01-01T10:00:00.000Z".to_owned(),
        updated_at: "2026-01-01T11:30:00.000Z".to_owned(),
    }
}

/// The same agent in a folder without a branch, alone on an implementation step.
pub(crate) fn detached_agent() -> AgentRecord {
    AgentRecord {
        branch: String::new(),
        workspaces: Vec::new(),
        mr_url: None,
        note: None,
        steps: vec![step(
            "d1",
            "implementation",
            "Implémentation",
            StepState::Current,
            "sonnet",
        )],
        ..review_agent()
    }
}

pub(crate) fn sample_repos() -> Vec<RepoView> {
    let repo = |name: &str, note: &str| RepoView {
        name: name.to_owned(),
        path: format!("/repos/{name}"),
        ticket_prefixes: Vec::new(),
        base_branch: None,
        notes: vec![note.to_owned()],
        setup: None,
    };
    vec![repo("shop", "API sur api.test"), repo("other", "ignorée")]
}
