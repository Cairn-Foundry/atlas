// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The tables of the registry, and what brings a database created by an earlier version up to date.

use rusqlite::{Connection, params};

use crate::contract::StepState;

use super::{NewStep, insert_steps};

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS agents (
  id TEXT PRIMARY KEY,
  ticket_ref TEXT NOT NULL,
  title TEXT NOT NULL,
  repo_path TEXT NOT NULL,
  worktree_path TEXT NOT NULL,
  branch TEXT NOT NULL,
  base_branch TEXT NOT NULL DEFAULT 'main',
  step_id TEXT NOT NULL,
  status TEXT NOT NULL,
  status_reason TEXT,
  step_started_at TEXT NOT NULL,
  step_wait_ms INTEGER NOT NULL DEFAULT 0,
  waiting_since TEXT,
  mr_url TEXT,
  live_session_id TEXT,
  approval_requested INTEGER NOT NULL DEFAULT 0,
  threshold_alerted INTEGER NOT NULL DEFAULT 0,
  note TEXT,
  stop_blocks INTEGER NOT NULL DEFAULT 0,
  owns_worktree INTEGER NOT NULL DEFAULT 1,
  archived_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL REFERENCES agents(id),
  role TEXT NOT NULL DEFAULT '',
  group_id TEXT NOT NULL DEFAULT '',
  model TEXT NOT NULL,
  step_id TEXT NOT NULL,
  started_at TEXT NOT NULL,
  ended_at TEXT,
  outcome TEXT
);
CREATE TABLE IF NOT EXISTS agent_steps (
  id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL REFERENCES agents(id),
  position INTEGER NOT NULL,
  brick_id TEXT NOT NULL,
  name TEXT NOT NULL,
  model TEXT NOT NULL,
  state TEXT NOT NULL,
  pass INTEGER NOT NULL DEFAULT 1,
  started_at TEXT,
  ended_at TEXT,
  wait_ms INTEGER NOT NULL DEFAULT 0,
  input_tokens INTEGER,
  output_tokens INTEGER,
  cache_read_tokens INTEGER,
  cache_write_tokens INTEGER,
  output TEXT,
  session_id TEXT,
  head_sha TEXT,
  findings INTEGER
);
CREATE INDEX IF NOT EXISTS agent_steps_agent ON agent_steps(agent_id, position);
CREATE TABLE IF NOT EXISTS agent_workspaces (
  id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL REFERENCES agents(id),
  repo_name TEXT NOT NULL,
  repo_path TEXT NOT NULL,
  worktree_path TEXT NOT NULL,
  branch TEXT NOT NULL,
  base_branch TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS orchestrator_runs (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  model TEXT NOT NULL,
  input_tokens INTEGER NOT NULL,
  output_tokens INTEGER NOT NULL,
  cache_read_tokens INTEGER NOT NULL,
  cache_write_tokens INTEGER NOT NULL,
  turns INTEGER NOT NULL,
  duration_ms INTEGER NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS sessions_agent ON sessions(agent_id, started_at);
CREATE TABLE IF NOT EXISTS messages (
  id TEXT PRIMARY KEY,
  role TEXT NOT NULL,
  text TEXT NOT NULL,
  agent_id TEXT,
  kind TEXT NOT NULL DEFAULT 'text',
  card TEXT,
  actions TEXT,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS broadcasts (
  id TEXT PRIMARY KEY,
  text TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS deliveries (
  broadcast_id TEXT NOT NULL REFERENCES broadcasts(id),
  agent_id TEXT NOT NULL REFERENCES agents(id),
  PRIMARY KEY (broadcast_id, agent_id)
);
CREATE TABLE IF NOT EXISTS repos (
  name TEXT PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  ticket_prefixes TEXT NOT NULL DEFAULT '[]',
  base_branch TEXT,
  notes TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS kv (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
";

struct LegacyStep {
    legacy_id: &'static str,
    brick_id: &'static str,
    name: &'static str,
    model: &'static str,
}

/// Steps of the single workflow Atlas had before the catalogue, for the agents created then.
const LEGACY_STEPS: [LegacyStep; 6] = [
    LegacyStep {
        legacy_id: "qualification",
        brick_id: "qualification",
        name: "Qualification",
        model: "sonnet",
    },
    LegacyStep {
        legacy_id: "plan",
        brick_id: "plan",
        name: "Plan",
        model: "opus",
    },
    LegacyStep {
        legacy_id: "implementation",
        brick_id: "implementation",
        name: "Implémentation",
        model: "sonnet",
    },
    LegacyStep {
        legacy_id: "tests",
        brick_id: "tests",
        name: "Tests",
        model: "sonnet",
    },
    LegacyStep {
        legacy_id: "review",
        brick_id: "review",
        name: "Review",
        model: "opus",
    },
    // The verification step is gone: an agent caught in it goes straight to the ship.
    LegacyStep {
        legacy_id: "verification",
        brick_id: "ship",
        name: "Ship",
        model: "sonnet",
    },
];
const LEGACY_SHIP_INDEX: usize = LEGACY_STEPS.len() - 1;

fn add_column(db: &Connection, table: &str, column: &str, definition: &str) -> rusqlite::Result<bool> {
    let columns: Vec<String> = db
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |row| row.get::<_, String>("name"))?
        .collect::<rusqlite::Result<_>>()?;
    if columns.iter().any(|existing| existing == column) {
        return Ok(false);
    }
    db.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"))?;
    Ok(true)
}

/// Gives the agents of the single workflow their own steps, at the brick matching where they were.
fn migrate_legacy_agents(db: &Connection) -> rusqlite::Result<()> {
    struct Legacy {
        id: String,
        status: String,
        step_id: String,
        step_started_at: String,
        summary: Option<String>,
    }
    let agents: Vec<Legacy> = db
        .prepare("SELECT * FROM agents WHERE id NOT IN (SELECT agent_id FROM agent_steps)")?
        .query_map([], |row| {
            Ok(Legacy {
                id: row.get("id")?,
                status: row.get("status")?,
                step_id: row.get("step_id")?,
                step_started_at: row.get("step_started_at")?,
                // Only the databases of the single workflow have this column.
                summary: row.get::<_, Option<String>>("last_summary").ok().flatten(),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    for agent in agents {
        // All of an agent's steps or none: a half-migrated agent would never be picked up again.
        let transaction = db.unchecked_transaction()?;
        let is_done = agent.status == "done";
        let legacy_index = LEGACY_STEPS.iter().position(|step| step.legacy_id == agent.step_id);
        let current_index = if is_done {
            LEGACY_SHIP_INDEX
        } else {
            legacy_index.unwrap_or(LEGACY_SHIP_INDEX)
        };
        let steps: Vec<NewStep> = LEGACY_STEPS
            .iter()
            .enumerate()
            .map(|(index, step)| NewStep {
                brick_id: step.brick_id.to_owned(),
                name: step.name.to_owned(),
                model: step.model.to_owned(),
                state: match index.cmp(&current_index) {
                    std::cmp::Ordering::Less => StepState::External,
                    std::cmp::Ordering::Equal if is_done => StepState::Done,
                    std::cmp::Ordering::Equal => StepState::Current,
                    std::cmp::Ordering::Greater => StepState::Upcoming,
                },
                pass: 1,
            })
            .collect();
        insert_steps(db, &agent.id, &steps, 0, &agent.step_started_at)?;
        db.execute(
            "UPDATE agents SET step_id = ? WHERE id = ?",
            params![LEGACY_STEPS[current_index].brick_id, agent.id],
        )?;
        // The single summary the agent carried becomes the section of the step it came from.
        if let Some(summary) = agent.summary.filter(|_| current_index > 0) {
            let output = serde_json::json!({ "summary": summary }).to_string();
            db.execute(
                "UPDATE agent_steps SET output = ? WHERE agent_id = ? AND position = ?",
                params![output, agent.id, current_index - 1],
            )?;
        }
        transaction.commit()?;
    }
    Ok(())
}

/// Columns and rows added after the first release, for databases created before them.
pub(super) fn migrate(db: &Connection) -> rusqlite::Result<()> {
    add_column(db, "agents", "base_branch", "TEXT NOT NULL DEFAULT 'main'")?;
    add_column(db, "agents", "owns_worktree", "INTEGER NOT NULL DEFAULT 1")?;
    add_column(db, "agents", "archived_at", "TEXT")?;
    add_column(db, "messages", "kind", "TEXT NOT NULL DEFAULT 'text'")?;
    add_column(db, "messages", "card", "TEXT")?;
    add_column(db, "messages", "actions", "TEXT")?;
    add_column(db, "repos", "notes", "TEXT NOT NULL DEFAULT '[]'")?;
    add_column(db, "repos", "setup", "TEXT")?;
    if add_column(db, "sessions", "group_id", "TEXT NOT NULL DEFAULT ''")? {
        // Sessions of the single workflow: authors kept one session, reviewers a fresh one per pass.
        db.execute_batch("UPDATE sessions SET group_id = CASE role WHEN 'reviewer' THEN 'review' ELSE 'build' END")?;
    }
    migrate_legacy_agents(db)
}
