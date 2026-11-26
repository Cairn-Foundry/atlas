// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Persistence for agents, sessions, chat and broadcasts: one SQLite file, synchronous access.
//!
//! The schema and its migrations go back to the first version of Atlas: a registry created by any
//! earlier version opens as it is.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use uuid::Uuid;

use crate::clock::{Clock, iso};
use crate::contract::{
    AgentStatus, ChatMessage, MessageAction, MessageCard, MessageKind, MessageRole, RepoView, SessionView, StepOutput,
    StepState, StepUsage,
};
use crate::error::{Error, Result};
use crate::language::tr;

mod rows;
mod schema;
#[cfg(test)]
pub(crate) mod tests;

pub use rows::to_agent_view;
use rows::{to_agent, to_message, to_repo, to_session, to_step};
use schema::{SCHEMA, migrate};

/// A step of an agent's workflow, with what the daemon keeps besides the public view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRecord {
    pub id: String,
    pub brick_id: String,
    pub name: String,
    pub state: StepState,
    /// Review pass, 1 for anything that is not a repeated review.
    pub pass: u32,
    pub model: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub wait_ms: i64,
    pub usage: Option<StepUsage>,
    /// Fields handed to `complete_step`: the step's section of the agent's file.
    pub output: Option<StepOutput>,
    /// Session the step last ran in.
    pub session_id: Option<String>,
    /// Commit the worktree was at when the step ended; the next review pass starts from it.
    pub head_sha: Option<String>,
    /// Review steps: findings at the threshold reported by the pass.
    pub findings: Option<u32>,
}

/// Another project the agent also works in, in a worktree on the branch of its work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraWorkspace {
    pub repo_name: String,
    pub repo_path: String,
    pub worktree_path: String,
    pub branch: String,
    pub base_branch: String,
}

/// What goes with the text of a chat message.
#[derive(Debug, Clone, Default)]
pub struct MessageExtras {
    pub kind: Option<MessageKind>,
    pub card: Option<MessageCard>,
    pub actions: Vec<MessageAction>,
}

/// Internal agent state: the public view plus the bookkeeping the daemon needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRecord {
    pub id: String,
    pub ticket_ref: String,
    pub title: String,
    pub repo_name: String,
    pub repo_path: String,
    pub worktree_path: String,
    pub branch: String,
    /// Branch the agent's work is compared to in review.
    pub base_branch: String,
    pub step_id: String,
    pub steps: Vec<StepRecord>,
    pub workspaces: Vec<ExtraWorkspace>,
    pub status: AgentStatus,
    pub status_reason: Option<String>,
    pub step_started_at: String,
    /// Milliseconds spent waiting for the master during the current step, excluded from thresholds.
    pub step_wait_ms: i64,
    pub waiting_since: Option<String>,
    pub mr_url: Option<String>,
    pub live_session_id: Option<String>,
    pub approval_requested: bool,
    pub threshold_alerted: bool,
    pub note: Option<String>,
    /// Consecutive Stop-hook refusals, reset whenever the agent makes progress.
    pub stop_blocks: u32,
    /// False when the agent works in a folder Atlas did not create: it is never removed.
    pub owns_worktree: bool,
    /// ISO date the agent left the board, none while it is on it.
    pub archived_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl AgentRecord {
    pub fn current_step(&self) -> Option<&StepRecord> {
        self.steps.iter().find(|step| step.state == StepState::Current)
    }

    pub fn upcoming_steps(&self) -> impl Iterator<Item = &StepRecord> {
        self.steps.iter().filter(|step| step.state == StepState::Upcoming)
    }
}

impl StepRecord {
    /// A step whose result the next ones can read: done in Atlas, or declared done outside it.
    pub fn is_finished(&self) -> bool {
        matches!(self.state, StepState::Done | StepState::External)
    }
}

/// What changes on an agent; a field left at none keeps its value.
#[derive(Debug, Clone, Default)]
pub struct AgentPatch {
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub base_branch: Option<String>,
    pub step_id: Option<String>,
    pub status: Option<AgentStatus>,
    pub status_reason: Option<Option<String>>,
    pub step_started_at: Option<String>,
    pub step_wait_ms: Option<i64>,
    pub waiting_since: Option<Option<String>>,
    pub mr_url: Option<Option<String>>,
    pub live_session_id: Option<Option<String>>,
    pub approval_requested: Option<bool>,
    pub threshold_alerted: Option<bool>,
    pub stop_blocks: Option<u32>,
    pub owns_worktree: Option<bool>,
    pub archived_at: Option<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewStep {
    pub brick_id: String,
    pub name: String,
    pub model: String,
    pub state: StepState,
    pub pass: u32,
}

/// What changes on a step; a field left at none keeps its value.
#[derive(Debug, Clone, Default)]
pub struct StepPatch {
    pub state: Option<StepState>,
    pub started_at: Option<Option<String>>,
    pub ended_at: Option<Option<String>>,
    pub wait_ms: Option<i64>,
    pub usage: Option<Option<StepUsage>>,
    pub output: Option<Option<StepOutput>>,
    pub session_id: Option<Option<String>>,
    pub head_sha: Option<Option<String>>,
    pub findings: Option<Option<u32>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestratorRunKind {
    Chat,
    Approval,
    Anomaly,
}

impl OrchestratorRunKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Approval => "approval",
            Self::Anomaly => "anomaly",
        }
    }
}

/// One `claude -p` run of the orchestrator, kept to follow what supervising costs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrchestratorRun {
    pub kind: OrchestratorRunKind,
    pub model: String,
    pub usage: StepUsage,
    pub turns: u32,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct NewAgent {
    pub ticket_ref: String,
    pub title: String,
    pub repo_path: String,
    pub worktree_path: String,
    pub branch: String,
    pub base_branch: String,
    pub note: Option<String>,
    pub owns_worktree: bool,
    pub mr_url: Option<String>,
    /// The whole workflow; exactly one step is `current`.
    pub steps: Vec<NewStep>,
}

#[derive(Debug, Clone)]
pub struct NewSession {
    pub id: String,
    pub agent_id: String,
    pub group: String,
    pub model: String,
    pub step_id: String,
}

/// A project to add or replace; its memory and setup have their own writers.
#[derive(Debug, Clone)]
pub struct RepoRecord {
    pub name: String,
    pub path: String,
    pub ticket_prefixes: Vec<String>,
    pub base_branch: Option<String>,
}

/// Agents with the name of their project, null when the repository is not a declared project.
const SELECT_AGENTS: &str =
    "SELECT agents.*, repos.name AS repo_name FROM agents LEFT JOIN repos ON repos.path = agents.repo_path";

const DEFAULT_MESSAGE_LIMIT: usize = 200;
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

fn insert_steps(
    db: &Connection,
    agent_id: &str,
    steps: &[NewStep],
    first_position: usize,
    stamp: &str,
) -> rusqlite::Result<()> {
    let mut insert = db.prepare(
        "INSERT INTO agent_steps (id, agent_id, position, brick_id, name, model, state, pass, started_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )?;
    for (index, step) in steps.iter().enumerate() {
        let started_at = (step.state == StepState::Current).then_some(stamp);
        insert.execute(params![
            new_id(),
            agent_id,
            first_position + index,
            step.brick_id,
            step.name,
            step.model,
            step.state.as_str(),
            step.pass,
            started_at
        ])?;
    }
    Ok(())
}

fn steps_of(db: &Connection, agent_id: &str) -> rusqlite::Result<Vec<StepRecord>> {
    db.prepare("SELECT * FROM agent_steps WHERE agent_id = ? ORDER BY position")?
        .query_map([agent_id], to_step)?
        .collect()
}

fn workspaces_of(db: &Connection, agent_id: &str) -> rusqlite::Result<Vec<ExtraWorkspace>> {
    db.prepare("SELECT * FROM agent_workspaces WHERE agent_id = ? ORDER BY created_at, rowid")?
        .query_map([agent_id], |row| {
            Ok(ExtraWorkspace {
                repo_name: row.get("repo_name")?,
                repo_path: row.get("repo_path")?,
                worktree_path: row.get("worktree_path")?,
                branch: row.get("branch")?,
                base_branch: row.get("base_branch")?,
            })
        })?
        .collect()
}

fn agent_of(db: &Connection, id: &str) -> rusqlite::Result<Option<AgentRecord>> {
    let steps = steps_of(db, id)?;
    let workspaces = workspaces_of(db, id)?;
    db.query_row(&format!("{SELECT_AGENTS} WHERE agents.id = ?"), [id], |row| {
        to_agent(row, steps, workspaces)
    })
    .optional()
}

fn repos_of(db: &Connection) -> rusqlite::Result<Vec<RepoView>> {
    db.prepare("SELECT * FROM repos ORDER BY name")?
        .query_map([], to_repo)?
        .collect()
}

struct StoreInner {
    db: Mutex<Connection>,
    now: Clock,
}

/// The registry. Cheap to clone: every clone reads and writes the same database.
#[derive(Clone)]
pub struct Store(Arc<StoreInner>);

impl Store {
    pub fn open(path: &Path, now: Clock) -> Result<Self> {
        Self::with_connection(Connection::open(path)?, now)
    }

    #[cfg(test)]
    pub fn in_memory(now: Clock) -> Result<Self> {
        Self::with_connection(Connection::open_in_memory()?, now)
    }

    fn with_connection(db: Connection, now: Clock) -> Result<Self> {
        // A daemon replaced by a newer one may still be closing the registry when the next opens it.
        db.busy_timeout(BUSY_TIMEOUT)?;
        db.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        db.execute_batch(SCHEMA)?;
        migrate(&db)?;
        Ok(Self(Arc::new(StoreInner {
            db: Mutex::new(db),
            now,
        })))
    }

    fn db(&self) -> MutexGuard<'_, Connection> {
        self.0.db.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn stamp(&self) -> String {
        iso((self.0.now)())
    }

    pub fn create_agent(&self, input: &NewAgent) -> Result<AgentRecord> {
        let Some(current) = input.steps.iter().find(|step| step.state == StepState::Current) else {
            return Err(Error::internal(&tr!(
                "un agent démarre sur une étape courante",
                "an agent starts on a current step"
            )));
        };
        let id = new_id();
        let stamp = self.stamp();
        {
            let db = self.db();
            // The agent and its steps go together: an agent without steps is one nobody can move.
            let transaction = db.unchecked_transaction()?;
            db.execute(
                "INSERT INTO agents (id, ticket_ref, title, repo_path, worktree_path, branch, base_branch, step_id, status, step_started_at, note, owns_worktree, mr_url, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'working', ?, ?, ?, ?, ?, ?)",
                params![
                    id,
                    input.ticket_ref,
                    input.title,
                    input.repo_path,
                    input.worktree_path,
                    input.branch,
                    input.base_branch,
                    current.brick_id,
                    stamp,
                    input.note,
                    input.owns_worktree,
                    input.mr_url,
                    stamp,
                    stamp
                ],
            )?;
            insert_steps(&db, &id, &input.steps, 0, &stamp)?;
            transaction.commit()?;
        }
        self.require_agent(&id)
    }

    pub fn add_workspace(&self, agent_id: &str, workspace: &ExtraWorkspace) -> Result<()> {
        self.db().execute(
            "INSERT INTO agent_workspaces (id, agent_id, repo_name, repo_path, worktree_path, branch, base_branch, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                new_id(),
                agent_id,
                workspace.repo_name,
                workspace.repo_path,
                workspace.worktree_path,
                workspace.branch,
                workspace.base_branch,
                self.stamp()
            ],
        )?;
        Ok(())
    }

    pub fn get_agent(&self, id: &str) -> Result<Option<AgentRecord>> {
        Ok(agent_of(&self.db(), id)?)
    }

    pub fn require_agent(&self, id: &str) -> Result<AgentRecord> {
        self.get_agent(id)?
            .ok_or_else(|| Error::internal(tr!("agent inconnu : {id}", "unknown agent: {id}")))
    }

    /// Agents on the board, oldest first; `include_archived` adds the ones that left it.
    pub fn list_agents(&self, include_archived: bool) -> Result<Vec<AgentRecord>> {
        let filter = if include_archived {
            ""
        } else {
            "WHERE agents.archived_at IS NULL"
        };
        let db = self.db();
        let ids: Vec<String> = db
            .prepare(&format!(
                "SELECT agents.id FROM agents {filter} ORDER BY agents.created_at"
            ))?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut agents = Vec::with_capacity(ids.len());
        for id in ids {
            agents.extend(agent_of(&db, &id)?);
        }
        Ok(agents)
    }

    pub fn update_agent(&self, id: &str, patch: AgentPatch) -> Result<AgentRecord> {
        let mut assignments: Vec<&str> = Vec::new();
        let mut values: Vec<Value> = Vec::new();
        let mut set = |column: &'static str, value: Value| {
            assignments.push(column);
            values.push(value);
        };
        let text = |value: Option<String>| value.map_or(Value::Null, Value::Text);
        if let Some(value) = patch.worktree_path {
            set("worktree_path = ?", Value::Text(value));
        }
        if let Some(value) = patch.branch {
            set("branch = ?", Value::Text(value));
        }
        if let Some(value) = patch.base_branch {
            set("base_branch = ?", Value::Text(value));
        }
        if let Some(value) = patch.step_id {
            set("step_id = ?", Value::Text(value));
        }
        if let Some(value) = patch.status {
            set("status = ?", Value::Text(value.as_str().to_owned()));
        }
        if let Some(value) = patch.status_reason {
            set("status_reason = ?", text(value));
        }
        if let Some(value) = patch.step_started_at {
            set("step_started_at = ?", Value::Text(value));
        }
        if let Some(value) = patch.step_wait_ms {
            set("step_wait_ms = ?", Value::Integer(value));
        }
        if let Some(value) = patch.waiting_since {
            set("waiting_since = ?", text(value));
        }
        if let Some(value) = patch.mr_url {
            set("mr_url = ?", text(value));
        }
        if let Some(value) = patch.live_session_id {
            set("live_session_id = ?", text(value));
        }
        if let Some(value) = patch.approval_requested {
            set("approval_requested = ?", Value::Integer(value.into()));
        }
        if let Some(value) = patch.threshold_alerted {
            set("threshold_alerted = ?", Value::Integer(value.into()));
        }
        if let Some(value) = patch.stop_blocks {
            set("stop_blocks = ?", Value::Integer(value.into()));
        }
        if let Some(value) = patch.owns_worktree {
            set("owns_worktree = ?", Value::Integer(value.into()));
        }
        if let Some(value) = patch.archived_at {
            set("archived_at = ?", text(value));
        }
        set("updated_at = ?", Value::Text(self.stamp()));
        values.push(Value::Text(id.to_owned()));
        self.db().execute(
            &format!("UPDATE agents SET {} WHERE id = ?", assignments.join(", ")),
            params_from_iter(values),
        )?;
        self.require_agent(id)
    }

    pub fn update_step(&self, step_id: &str, patch: StepPatch) -> Result<()> {
        let mut assignments: Vec<&str> = Vec::new();
        let mut values: Vec<Value> = Vec::new();
        let mut set = |column: &'static str, value: Value| {
            assignments.push(column);
            values.push(value);
        };
        let text = |value: Option<String>| value.map_or(Value::Null, Value::Text);
        let count = |value: Option<u64>| {
            value.map_or(Value::Null, |count| {
                Value::Integer(i64::try_from(count).unwrap_or(i64::MAX))
            })
        };
        if let Some(value) = patch.state {
            set("state = ?", Value::Text(value.as_str().to_owned()));
        }
        if let Some(value) = patch.started_at {
            set("started_at = ?", text(value));
        }
        if let Some(value) = patch.ended_at {
            set("ended_at = ?", text(value));
        }
        if let Some(value) = patch.wait_ms {
            set("wait_ms = ?", Value::Integer(value));
        }
        if let Some(value) = patch.session_id {
            set("session_id = ?", text(value));
        }
        if let Some(value) = patch.head_sha {
            set("head_sha = ?", text(value));
        }
        if let Some(value) = patch.findings {
            set(
                "findings = ?",
                value.map_or(Value::Null, |findings| Value::Integer(findings.into())),
            );
        }
        if let Some(value) = patch.output {
            set(
                "output = ?",
                text(value.map(|output| serde_json::to_string(&output)).transpose()?),
            );
        }
        if let Some(usage) = patch.usage {
            set("input_tokens = ?", count(usage.map(|usage| usage.input_tokens)));
            set("output_tokens = ?", count(usage.map(|usage| usage.output_tokens)));
            set(
                "cache_read_tokens = ?",
                count(usage.map(|usage| usage.cache_read_tokens)),
            );
            set(
                "cache_write_tokens = ?",
                count(usage.map(|usage| usage.cache_write_tokens)),
            );
        }
        if assignments.is_empty() {
            return Ok(());
        }
        values.push(Value::Text(step_id.to_owned()));
        self.db().execute(
            &format!("UPDATE agent_steps SET {} WHERE id = ?", assignments.join(", ")),
            params_from_iter(values),
        )?;
        Ok(())
    }

    pub fn insert_step_after(&self, agent_id: &str, after_step_id: &str, step: &NewStep) -> Result<()> {
        let db = self.db();
        let transaction = db.unchecked_transaction()?;
        let position: Option<usize> = db
            .query_row(
                "SELECT position FROM agent_steps WHERE id = ? AND agent_id = ?",
                params![after_step_id, agent_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(position) = position else {
            return Err(Error::internal(tr!(
                "étape inconnue : {after_step_id}",
                "unknown step: {after_step_id}"
            )));
        };
        db.execute(
            "UPDATE agent_steps SET position = position + 1 WHERE agent_id = ? AND position > ?",
            params![agent_id, position],
        )?;
        insert_steps(&db, agent_id, std::slice::from_ref(step), position + 1, &self.stamp())?;
        transaction.commit()?;
        Ok(())
    }

    /// Replaces the steps the agent has not started: `dropped` stay in its history as skipped, then
    /// come the new `upcoming` ones.
    pub fn replace_upcoming_steps(&self, agent_id: &str, dropped: &[NewStep], upcoming: &[NewStep]) -> Result<()> {
        let db = self.db();
        // The steps dropped and the ones that replace them go together: a failure leaves the workflow as it was.
        let transaction = db.unchecked_transaction()?;
        db.execute(
            "DELETE FROM agent_steps WHERE agent_id = ? AND state = ?",
            params![agent_id, StepState::Upcoming.as_str()],
        )?;
        let last: i64 = db.query_row(
            "SELECT COALESCE(MAX(position), -1) FROM agent_steps WHERE agent_id = ?",
            [agent_id],
            |row| row.get(0),
        )?;
        let first = usize::try_from(last + 1).unwrap_or(0);
        let steps: Vec<NewStep> = dropped.iter().chain(upcoming).cloned().collect();
        insert_steps(&db, agent_id, &steps, first, &self.stamp())?;
        transaction.commit()?;
        Ok(())
    }

    pub fn create_session(&self, input: &NewSession) -> Result<SessionView> {
        self.db().execute(
            // `role` only exists for databases created before session groups: it has no default there.
            "INSERT INTO sessions (id, agent_id, role, group_id, model, step_id, started_at) VALUES (?, ?, '', ?, ?, ?, ?)",
            params![input.id, input.agent_id, input.group, input.model, input.step_id, self.stamp()],
        )?;
        self.get_session(&input.id)?
            .ok_or_else(|| Error::internal(tr!("session inconnue : {}", "unknown session: {}", input.id)))
    }

    /// Makes the agent's sessions unusable for a later step: they belong to a folder it has left.
    pub fn retire_sessions(&self, agent_id: &str) -> Result<()> {
        self.db().execute(
            "UPDATE sessions SET group_id = group_id || ':left' WHERE agent_id = ? AND group_id NOT LIKE '%:left'",
            [agent_id],
        )?;
        Ok(())
    }

    /// The agent's latest session of a group and model: the one a later step of that group resumes.
    pub fn find_group_session(&self, agent_id: &str, group: &str, model: &str) -> Result<Option<SessionView>> {
        Ok(self
            .db()
            .query_row(
                "SELECT * FROM sessions WHERE agent_id = ? AND group_id = ? AND model = ? ORDER BY started_at DESC, rowid DESC LIMIT 1",
                params![agent_id, group, model],
                to_session,
            )
            .optional()?)
    }

    pub fn end_session(&self, id: &str, outcome: &str) -> Result<()> {
        self.db().execute(
            "UPDATE sessions SET ended_at = ?, outcome = ? WHERE id = ? AND ended_at IS NULL",
            params![self.stamp(), outcome, id],
        )?;
        Ok(())
    }

    pub fn reopen_session(&self, id: &str) -> Result<()> {
        self.db()
            .execute("UPDATE sessions SET ended_at = NULL, outcome = NULL WHERE id = ?", [id])?;
        Ok(())
    }

    pub fn get_session(&self, id: &str) -> Result<Option<SessionView>> {
        Ok(self
            .db()
            .query_row("SELECT * FROM sessions WHERE id = ?", [id], to_session)
            .optional()?)
    }

    pub fn list_sessions(&self, agent_id: &str) -> Result<Vec<SessionView>> {
        let db = self.db();
        let sessions = db
            .prepare("SELECT * FROM sessions WHERE agent_id = ? ORDER BY started_at")?
            .query_map([agent_id], to_session)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(sessions)
    }

    pub fn add_message(
        &self,
        role: MessageRole,
        text: &str,
        agent_id: Option<&str>,
        extras: MessageExtras,
    ) -> Result<ChatMessage> {
        let id = new_id();
        let db = self.db();
        db.execute(
            "INSERT INTO messages (id, role, text, agent_id, kind, card, actions, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                id,
                role.as_str(),
                text,
                agent_id,
                extras.kind.unwrap_or(MessageKind::Text).as_str(),
                extras.card.map(|card| serde_json::to_string(&card)).transpose()?,
                serde_json::to_string(&extras.actions)?,
                self.stamp()
            ],
        )?;
        Ok(db.query_row("SELECT * FROM messages WHERE id = ?", [&id], to_message)?)
    }

    pub fn get_message(&self, id: &str) -> Result<Option<ChatMessage>> {
        Ok(self
            .db()
            .query_row("SELECT * FROM messages WHERE id = ?", [id], to_message)
            .optional()?)
    }

    /// Replaces the buttons of a message, such as one whose proposal the master decided.
    pub fn set_message_actions(&self, id: &str, actions: &[MessageAction]) -> Result<ChatMessage> {
        let db = self.db();
        db.execute(
            "UPDATE messages SET actions = ? WHERE id = ?",
            params![serde_json::to_string(actions)?, id],
        )?;
        Ok(db.query_row("SELECT * FROM messages WHERE id = ?", [id], to_message)?)
    }

    pub fn add_orchestrator_run(&self, run: &OrchestratorRun) -> Result<()> {
        self.db().execute(
            "INSERT INTO orchestrator_runs (id, kind, model, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, turns, duration_ms, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                new_id(),
                run.kind.as_str(),
                run.model,
                run.usage.input_tokens,
                run.usage.output_tokens,
                run.usage.cache_read_tokens,
                run.usage.cache_write_tokens,
                run.turns,
                run.duration_ms,
                self.stamp()
            ],
        )?;
        Ok(())
    }

    /// The latest messages, oldest first; `after` keeps only those created after that ISO date.
    pub fn list_messages(&self, after: Option<&str>) -> Result<Vec<ChatMessage>> {
        let db = self.db();
        let messages = db
            .prepare(
                "SELECT * FROM (SELECT *, rowid AS seq FROM messages WHERE created_at > ? ORDER BY created_at DESC, rowid DESC LIMIT ?)
                 ORDER BY created_at, seq",
            )?
            .query_map(params![after.unwrap_or(""), DEFAULT_MESSAGE_LIMIT], to_message)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(messages)
    }

    pub fn add_broadcast(&self, text: &str) -> Result<String> {
        let id = new_id();
        self.db().execute(
            "INSERT INTO broadcasts (id, text, created_at) VALUES (?, ?, ?)",
            params![id, text, self.stamp()],
        )?;
        Ok(id)
    }

    /// Broadcasts emitted after the agent was created and not yet delivered to it; marks them delivered.
    pub fn take_pending_broadcasts(&self, agent_id: &str) -> Result<Vec<String>> {
        let agent = self.require_agent(agent_id)?;
        let db = self.db();
        let pending: Vec<(String, String)> = db
            .prepare(
                "SELECT b.id, b.text FROM broadcasts b
                 WHERE b.created_at >= ? AND NOT EXISTS (SELECT 1 FROM deliveries d WHERE d.broadcast_id = b.id AND d.agent_id = ?)
                 ORDER BY b.created_at",
            )?
            .query_map(params![agent.created_at, agent_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut texts = Vec::with_capacity(pending.len());
        for (broadcast_id, text) in pending {
            db.execute(
                "INSERT OR IGNORE INTO deliveries (broadcast_id, agent_id) VALUES (?, ?)",
                params![broadcast_id, agent_id],
            )?;
            texts.push(text);
        }
        Ok(texts)
    }

    pub fn list_repos(&self) -> Result<Vec<RepoView>> {
        Ok(repos_of(&self.db())?)
    }

    /// Sets the command that installs a project's dependencies; false when the project is unknown.
    pub fn set_repo_setup(&self, name: &str, setup: Option<&str>) -> Result<bool> {
        Ok(self
            .db()
            .execute("UPDATE repos SET setup = ? WHERE name = ?", params![setup, name])?
            > 0)
    }

    /// Replaces the memory of a project; false when the project is unknown.
    pub fn set_repo_notes(&self, name: &str, notes: &[String]) -> Result<bool> {
        Ok(self.db().execute(
            "UPDATE repos SET notes = ? WHERE name = ?",
            params![serde_json::to_string(notes)?, name],
        )? > 0)
    }

    pub fn get_repo(&self, name: &str) -> Result<Option<RepoView>> {
        Ok(self.list_repos()?.into_iter().find(|repo| repo.name == name))
    }

    /// Adds or replaces a project by name. Its memory and setup are left as they are.
    pub fn save_repo(&self, repo: &RepoRecord) -> Result<RepoView> {
        let prefixes: Vec<String> = repo
            .ticket_prefixes
            .iter()
            .map(|prefix| prefix.to_uppercase())
            .collect();
        {
            let db = self.db();
            // A project renamed in place: the old name only goes if the new one is recorded.
            let transaction = db.unchecked_transaction()?;
            db.execute(
                "DELETE FROM repos WHERE path = ? AND name <> ?",
                params![repo.path, repo.name],
            )?;
            db.execute(
                "INSERT INTO repos (name, path, ticket_prefixes, base_branch, created_at) VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT(name) DO UPDATE SET path = excluded.path, ticket_prefixes = excluded.ticket_prefixes, base_branch = excluded.base_branch",
                params![repo.name, repo.path, serde_json::to_string(&prefixes)?, repo.base_branch, self.stamp()],
            )?;
            transaction.commit()?;
        }
        self.get_repo(&repo.name)?.ok_or_else(|| {
            Error::internal(tr!(
                "projet introuvable après enregistrement : {}",
                "project not found after it was recorded: {}",
                repo.name
            ))
        })
    }

    pub fn remove_repo(&self, name: &str) -> Result<bool> {
        Ok(self.db().execute("DELETE FROM repos WHERE name = ?", [name])? > 0)
    }

    pub fn get_value(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .db()
            .query_row("SELECT value FROM kv WHERE key = ?", [key], |row| row.get(0))
            .optional()?)
    }

    pub fn delete_value(&self, key: &str) -> Result<()> {
        self.db().execute("DELETE FROM kv WHERE key = ?", [key])?;
        Ok(())
    }

    pub fn set_value(&self, key: &str, value: &str) -> Result<()> {
        self.db().execute(
            "INSERT INTO kv (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}
