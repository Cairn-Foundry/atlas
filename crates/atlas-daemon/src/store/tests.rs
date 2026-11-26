// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The registry on a real SQLite database, in memory or in a file; also the records other tests start from.

use super::*;
use crate::clock::tests::TestClock;
use crate::contract::REPORT_FIELD;

pub(crate) fn step(brick_id: &str, state: StepState) -> NewStep {
    NewStep {
        brick_id: brick_id.to_owned(),
        name: brick_id.to_uppercase(),
        model: "sonnet".to_owned(),
        state,
        pass: 1,
    }
}

pub(crate) fn new_agent(ticket_ref: &str, steps: Vec<NewStep>) -> NewAgent {
    NewAgent {
        ticket_ref: ticket_ref.to_owned(),
        title: format!("Travail {ticket_ref}"),
        repo_path: "/repos/shop".to_owned(),
        worktree_path: format!("/worktrees/{ticket_ref}"),
        branch: format!("feat/{ticket_ref}/work"),
        base_branch: "main".to_owned(),
        note: None,
        owns_worktree: true,
        mr_url: None,
        steps,
    }
}

fn store() -> (Store, TestClock) {
    let clock = TestClock::new();
    (Store::in_memory(clock.clock()).expect("store"), clock)
}

fn workflow() -> Vec<NewStep> {
    vec![
        step("qualification", StepState::Current),
        step("plan", StepState::Upcoming),
        step("ship", StepState::Upcoming),
    ]
}

#[test]
fn creates_an_agent_on_its_current_step() {
    let (store, _) = store();
    let agent = store.create_agent(&new_agent("SHOP-1", workflow())).expect("agent");
    assert_eq!(
        (agent.status, agent.step_id.as_str()),
        (AgentStatus::Working, "qualification")
    );
    assert_eq!(agent.repo_name, "shop");
    assert_eq!(agent.steps[0].started_at.as_deref(), Some("2026-01-01T10:00:00.000Z"));
    assert_eq!(agent.steps[1].started_at, None);
    assert!(
        store
            .create_agent(&new_agent("SHOP-2", vec![step("plan", StepState::Upcoming)]))
            .is_err()
    );
}

#[test]
fn names_the_agent_after_its_declared_project() {
    let (store, _) = store();
    let repo = RepoRecord {
        name: "boutique".into(),
        path: "/repos/shop".into(),
        ticket_prefixes: vec!["shop".into()],
        base_branch: None,
    };
    assert_eq!(store.save_repo(&repo).expect("repo").ticket_prefixes, ["SHOP"]);
    assert_eq!(
        store
            .create_agent(&new_agent("SHOP-1", workflow()))
            .expect("agent")
            .repo_name,
        "boutique"
    );
}

#[test]
fn patches_only_the_given_fields_and_can_clear_one() {
    let (store, clock) = store();
    let agent = store.create_agent(&new_agent("SHOP-1", workflow())).expect("agent");
    clock.advance_minutes(1);
    let waiting = store
        .update_agent(
            &agent.id,
            AgentPatch {
                status: Some(AgentStatus::Waiting),
                status_reason: Some(Some("question".into())),
                ..AgentPatch::default()
            },
        )
        .expect("patch");
    assert_eq!(
        (waiting.status, waiting.status_reason.as_deref()),
        (AgentStatus::Waiting, Some("question"))
    );
    assert_eq!(waiting.updated_at, "2026-01-01T10:01:00.000Z");
    let cleared = store
        .update_agent(
            &agent.id,
            AgentPatch {
                status_reason: Some(None),
                ..AgentPatch::default()
            },
        )
        .expect("clear");
    assert_eq!((cleared.status, cleared.status_reason), (AgentStatus::Waiting, None));
}

#[test]
fn keeps_the_fields_of_a_step_in_the_order_they_were_handed() {
    let (store, _) = store();
    let agent = store.create_agent(&new_agent("SHOP-1", workflow())).expect("agent");
    let output: StepOutput = [("zeta", "last"), ("alpha", "first")]
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .into();
    let usage = StepUsage {
        input_tokens: 1,
        output_tokens: 2,
        cache_read_tokens: 3,
        cache_write_tokens: 4,
    };
    let patch = StepPatch {
        output: Some(Some(output)),
        usage: Some(Some(usage)),
        findings: Some(Some(2)),
        ..StepPatch::default()
    };
    store.update_step(&agent.steps[0].id, patch).expect("patch");
    let step = &store.require_agent(&agent.id).expect("agent").steps[0];
    assert_eq!(
        step.output.as_ref().expect("output").keys().collect::<Vec<_>>(),
        ["zeta", "alpha"]
    );
    assert_eq!((step.usage, step.findings), (Some(usage), Some(2)));
}

#[test]
fn shows_the_report_of_the_last_finished_step_that_wrote_one() {
    let (store, _) = store();
    let agent = store.create_agent(&new_agent("SHOP-1", workflow())).expect("agent");
    assert_eq!(to_agent_view(&agent).report, None);
    let output: StepOutput = [(REPORT_FIELD.to_owned(), "# Rapport".to_owned())].into();
    let patch = StepPatch {
        state: Some(StepState::Done),
        ended_at: Some(Some("2026-01-01T10:05:00.000Z".into())),
        output: Some(Some(output)),
        ..StepPatch::default()
    };
    store.update_step(&agent.steps[0].id, patch).expect("patch");
    let report = to_agent_view(&store.require_agent(&agent.id).expect("agent"))
        .report
        .expect("report");
    assert_eq!(
        (report.step_name.as_str(), report.markdown.as_str()),
        ("QUALIFICATION", "# Rapport")
    );
}

#[test]
fn inserts_a_step_after_another_and_shifts_the_rest() {
    let (store, _) = store();
    let agent = store.create_agent(&new_agent("SHOP-1", workflow())).expect("agent");
    store
        .insert_step_after(&agent.id, &agent.steps[0].id, &step("tests", StepState::Upcoming))
        .expect("insert");
    let bricks: Vec<String> = store
        .require_agent(&agent.id)
        .expect("agent")
        .steps
        .into_iter()
        .map(|step| step.brick_id)
        .collect();
    assert_eq!(bricks, ["qualification", "tests", "plan", "ship"]);
    assert!(
        store
            .insert_step_after(&agent.id, "ghost", &step("tests", StepState::Upcoming))
            .is_err()
    );
}

#[test]
fn replaces_the_steps_not_started_and_keeps_the_dropped_ones_as_skipped() {
    let (store, _) = store();
    let agent = store.create_agent(&new_agent("SHOP-1", workflow())).expect("agent");
    store
        .replace_upcoming_steps(
            &agent.id,
            &[step("plan", StepState::Skipped)],
            &[step("ship", StepState::Upcoming)],
        )
        .expect("replace");
    let steps: Vec<(String, StepState)> = store
        .require_agent(&agent.id)
        .expect("agent")
        .steps
        .into_iter()
        .map(|step| (step.brick_id, step.state))
        .collect();
    assert_eq!(
        steps,
        [
            ("qualification".to_owned(), StepState::Current),
            ("plan".to_owned(), StepState::Skipped),
            ("ship".to_owned(), StepState::Upcoming)
        ]
    );
}

#[test]
fn finds_the_latest_session_of_a_group_until_it_is_retired() {
    let (store, clock) = store();
    let agent = store.create_agent(&new_agent("SHOP-1", workflow())).expect("agent");
    let session = |id: &str, model: &str| NewSession {
        id: id.to_owned(),
        agent_id: agent.id.clone(),
        group: "build".to_owned(),
        model: model.to_owned(),
        step_id: "plan".to_owned(),
    };
    store.create_session(&session("s1", "sonnet")).expect("s1");
    clock.advance_minutes(1);
    store.create_session(&session("s2", "sonnet")).expect("s2");
    store.create_session(&session("s3", "opus")).expect("s3");
    assert_eq!(
        store
            .find_group_session(&agent.id, "build", "sonnet")
            .expect("find")
            .expect("session")
            .id,
        "s2"
    );
    store.end_session("s2", "exited").expect("end");
    store.end_session("s2", "again").expect("end twice");
    assert_eq!(
        store
            .get_session("s2")
            .expect("get")
            .expect("session")
            .outcome
            .as_deref(),
        Some("exited")
    );
    store.reopen_session("s2").expect("reopen");
    assert_eq!(store.get_session("s2").expect("get").expect("session").ended_at, None);
    store.retire_sessions(&agent.id).expect("retire");
    assert!(
        store
            .find_group_session(&agent.id, "build", "sonnet")
            .expect("find")
            .is_none()
    );
    assert_eq!(store.list_sessions(&agent.id).expect("list").len(), 3);
}

#[test]
fn lists_the_messages_after_a_date_with_their_card_and_actions() {
    let (store, clock) = store();
    store
        .add_message(MessageRole::User, "avant", None, MessageExtras::default())
        .expect("before");
    clock.advance_minutes(1);
    let cleared_at = iso((clock.clock())());
    clock.advance_minutes(1);
    let card = MessageCard {
        ticket_ref: "SHOP-1".into(),
        steps: vec![CardStep {
            name: "Plan".into(),
            state: StepState::Current,
        }],
    };
    let extras = MessageExtras {
        kind: Some(MessageKind::Launch),
        card: Some(card.clone()),
        actions: vec![MessageAction::Session],
    };
    store
        .add_message(MessageRole::Assistant, "après", None, extras)
        .expect("after");
    assert_eq!(store.list_messages(None).expect("all").len(), 2);
    let recent = store.list_messages(Some(&cleared_at)).expect("recent");
    assert_eq!(recent.len(), 1);
    assert_eq!(
        (recent[0].kind, recent[0].card.as_ref(), recent[0].actions.as_slice()),
        (MessageKind::Launch, Some(&card), [MessageAction::Session].as_slice())
    );
}

use crate::contract::CardStep;

#[test]
fn delivers_a_broadcast_once_and_only_to_agents_created_before_it() {
    let (store, clock) = store();
    let early = store.create_agent(&new_agent("SHOP-1", workflow())).expect("early");
    clock.advance_minutes(1);
    store.add_broadcast("stop pushing").expect("broadcast");
    clock.advance_minutes(1);
    let late = store.create_agent(&new_agent("SHOP-2", workflow())).expect("late");
    assert_eq!(
        store.take_pending_broadcasts(&early.id).expect("first"),
        ["stop pushing"]
    );
    assert!(store.take_pending_broadcasts(&early.id).expect("second").is_empty());
    assert!(store.take_pending_broadcasts(&late.id).expect("late").is_empty());
}

#[test]
fn keeps_the_memory_and_setup_of_a_project_when_it_is_saved_again() {
    let (store, _) = store();
    let repo = RepoRecord {
        name: "shop".into(),
        path: "/repos/shop".into(),
        ticket_prefixes: vec![],
        base_branch: None,
    };
    store.save_repo(&repo).expect("save");
    assert!(
        store
            .set_repo_notes("shop", &["host is api.test".to_owned()])
            .expect("notes")
    );
    assert!(store.set_repo_setup("shop", Some("pnpm install")).expect("setup"));
    assert!(!store.set_repo_setup("ghost", None).expect("unknown"));
    let saved = store
        .save_repo(&RepoRecord {
            base_branch: Some("develop".into()),
            ..repo.clone()
        })
        .expect("again");
    assert_eq!(
        (saved.notes.as_slice(), saved.setup.as_deref()),
        (["host is api.test".to_owned()].as_slice(), Some("pnpm install"))
    );
    assert_eq!(saved.base_branch.as_deref(), Some("develop"));
    // A project renamed at the same path replaces the old one.
    store
        .save_repo(&RepoRecord {
            name: "boutique".into(),
            ..repo
        })
        .expect("rename");
    assert_eq!(
        store
            .list_repos()
            .expect("list")
            .iter()
            .map(|repo| repo.name.as_str())
            .collect::<Vec<_>>(),
        ["boutique"]
    );
    assert!(store.remove_repo("boutique").expect("remove"));
}

#[test]
fn stores_values_by_key() {
    let (store, _) = store();
    assert_eq!(store.get_value("k").expect("get"), None);
    store.set_value("k", "1").expect("set");
    store.set_value("k", "2").expect("overwrite");
    assert_eq!(store.get_value("k").expect("get").as_deref(), Some("2"));
    store.delete_value("k").expect("delete");
    assert_eq!(store.get_value("k").expect("get"), None);
}

#[test]
fn gives_the_agents_of_the_single_workflow_their_own_steps() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("atlas.db");
    {
        let legacy = Connection::open(&path).expect("legacy");
        legacy
            .execute_batch(
                "CREATE TABLE agents (id TEXT PRIMARY KEY, ticket_ref TEXT NOT NULL, title TEXT NOT NULL, repo_path TEXT NOT NULL,
                   worktree_path TEXT NOT NULL, branch TEXT NOT NULL, step_id TEXT NOT NULL, status TEXT NOT NULL, status_reason TEXT,
                   step_started_at TEXT NOT NULL, step_wait_ms INTEGER NOT NULL DEFAULT 0, waiting_since TEXT, mr_url TEXT,
                   live_session_id TEXT, approval_requested INTEGER NOT NULL DEFAULT 0, threshold_alerted INTEGER NOT NULL DEFAULT 0,
                   note TEXT, stop_blocks INTEGER NOT NULL DEFAULT 0, last_summary TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
                 CREATE TABLE sessions (id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, role TEXT NOT NULL, model TEXT NOT NULL,
                   step_id TEXT NOT NULL, started_at TEXT NOT NULL, ended_at TEXT, outcome TEXT);
                 INSERT INTO agents (id, ticket_ref, title, repo_path, worktree_path, branch, step_id, status, step_started_at, last_summary, created_at, updated_at)
                   VALUES ('a1', 'SHOP-1', 'Old', '/repos/shop', '/wt', 'feat/x', 'tests', 'working', '2025-01-01T00:00:00.000Z', 'done so far', '2025-01-01T00:00:00.000Z', '2025-01-01T00:00:00.000Z'),
                          ('a2', 'SHOP-2', 'Gone', '/repos/shop', '/wt2', 'feat/y', 'verification', 'done', '2025-01-01T00:00:00.000Z', NULL, '2025-01-02T00:00:00.000Z', '2025-01-02T00:00:00.000Z');
                 INSERT INTO sessions VALUES ('s1', 'a1', 'reviewer', 'opus', 'review', '2025-01-01T00:00:00.000Z', NULL, NULL),
                                             ('s2', 'a1', 'author', 'sonnet', 'tests', '2025-01-01T00:00:00.000Z', NULL, NULL);",
            )
            .expect("legacy schema");
    }
    let store = Store::open(&path, TestClock::new().clock()).expect("store");
    let agent = store.require_agent("a1").expect("agent");
    let states: Vec<(&str, StepState)> = agent
        .steps
        .iter()
        .map(|step| (step.brick_id.as_str(), step.state))
        .collect();
    assert_eq!(
        states,
        [
            ("qualification", StepState::External),
            ("plan", StepState::External),
            ("implementation", StepState::External),
            ("tests", StepState::Current),
            ("review", StepState::Upcoming),
            ("ship", StepState::Upcoming)
        ]
    );
    assert_eq!(
        agent.steps[2]
            .output
            .as_ref()
            .and_then(|output| output.get("summary"))
            .map(String::as_str),
        Some("done so far")
    );
    assert_eq!((agent.base_branch.as_str(), agent.owns_worktree), ("main", true));
    let done = store.require_agent("a2").expect("done");
    assert_eq!((done.step_id.as_str(), done.steps[5].state), ("ship", StepState::Done));
    let groups: Vec<String> = store
        .list_sessions("a1")
        .expect("sessions")
        .into_iter()
        .map(|session| session.group)
        .collect();
    assert_eq!(groups, ["review", "build"]);
    // Opening it again changes nothing.
    drop(store);
    let reopened = Store::open(&path, TestClock::new().clock()).expect("reopen");
    assert_eq!(reopened.require_agent("a1").expect("agent").steps.len(), 6);
}
