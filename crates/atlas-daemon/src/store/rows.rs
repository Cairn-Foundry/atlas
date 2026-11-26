// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading the rows of the registry into records, and records into the views the clients receive.

use crate::language::tr;
use std::path::Path;

use rusqlite::Row;

use crate::contract::{
    AgentReport, AgentStatus, AgentStepView, AgentView, ChatMessage, MessageAction, MessageKind, MessageRole,
    REPORT_FIELD, RepoView, SessionView, StepState, StepUsage,
};

use super::{AgentRecord, ExtraWorkspace, StepRecord};

fn invalid(column: &str, value: &str) -> rusqlite::Error {
    rusqlite::Error::InvalidColumnName(tr!(
        "{column} : valeur inconnue ({value})",
        "{column}: unknown value ({value})"
    ))
}

fn json_column<T: serde::de::DeserializeOwned>(column: &str, source: &str) -> rusqlite::Result<T> {
    serde_json::from_str(source).map_err(|_| invalid(column, source))
}

pub(super) fn to_step(row: &Row<'_>) -> rusqlite::Result<StepRecord> {
    let state: String = row.get("state")?;
    let input_tokens: Option<u64> = row.get("input_tokens")?;
    let usage = match input_tokens {
        None => None,
        Some(input_tokens) => Some(StepUsage {
            input_tokens,
            output_tokens: row.get::<_, Option<u64>>("output_tokens")?.unwrap_or(0),
            cache_read_tokens: row.get::<_, Option<u64>>("cache_read_tokens")?.unwrap_or(0),
            cache_write_tokens: row.get::<_, Option<u64>>("cache_write_tokens")?.unwrap_or(0),
        }),
    };
    let output: Option<String> = row.get("output")?;
    Ok(StepRecord {
        id: row.get("id")?,
        brick_id: row.get("brick_id")?,
        name: row.get("name")?,
        state: StepState::parse(&state).ok_or_else(|| invalid("state", &state))?,
        pass: row.get("pass")?,
        model: row.get("model")?,
        started_at: row.get("started_at")?,
        ended_at: row.get("ended_at")?,
        wait_ms: row.get("wait_ms")?,
        usage,
        output: output.map(|source| json_column("output", &source)).transpose()?,
        session_id: row.get("session_id")?,
        head_sha: row.get("head_sha")?,
        findings: row.get("findings")?,
    })
}

fn folder_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

pub(super) fn to_agent(
    row: &Row<'_>,
    steps: Vec<StepRecord>,
    workspaces: Vec<ExtraWorkspace>,
) -> rusqlite::Result<AgentRecord> {
    let repo_path: String = row.get("repo_path")?;
    let status: String = row.get("status")?;
    Ok(AgentRecord {
        id: row.get("id")?,
        ticket_ref: row.get("ticket_ref")?,
        title: row.get("title")?,
        repo_name: row
            .get::<_, Option<String>>("repo_name")?
            .unwrap_or_else(|| folder_name(&repo_path)),
        repo_path,
        worktree_path: row.get("worktree_path")?,
        branch: row.get("branch")?,
        base_branch: row.get("base_branch")?,
        step_id: row.get("step_id")?,
        steps,
        workspaces,
        status: AgentStatus::parse(&status).ok_or_else(|| invalid("status", &status))?,
        status_reason: row.get("status_reason")?,
        step_started_at: row.get("step_started_at")?,
        step_wait_ms: row.get("step_wait_ms")?,
        waiting_since: row.get("waiting_since")?,
        mr_url: row.get("mr_url")?,
        live_session_id: row.get("live_session_id")?,
        approval_requested: row.get("approval_requested")?,
        threshold_alerted: row.get("threshold_alerted")?,
        note: row.get("note")?,
        stop_blocks: row.get("stop_blocks")?,
        owns_worktree: row.get("owns_worktree")?,
        archived_at: row.get("archived_at")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub(super) fn to_session(row: &Row<'_>) -> rusqlite::Result<SessionView> {
    Ok(SessionView {
        id: row.get("id")?,
        agent_id: row.get("agent_id")?,
        group: row.get("group_id")?,
        model: row.get("model")?,
        step_id: row.get("step_id")?,
        started_at: row.get("started_at")?,
        ended_at: row.get("ended_at")?,
        outcome: row.get("outcome")?,
    })
}

pub(super) fn to_message(row: &Row<'_>) -> rusqlite::Result<ChatMessage> {
    let role: String = row.get("role")?;
    let kind: String = row.get("kind")?;
    let card: Option<String> = row.get("card")?;
    let actions: Option<String> = row.get("actions")?;
    let agent_id: Option<String> = row.get("agent_id")?;
    let actions = match actions {
        Some(source) => json_column("actions", &source)?,
        // Messages stored before actions existed offered the session of their agent.
        None if agent_id.is_some() => vec![MessageAction::Session],
        None => Vec::new(),
    };
    Ok(ChatMessage {
        id: row.get("id")?,
        role: if role == "user" {
            MessageRole::User
        } else {
            MessageRole::Assistant
        },
        kind: MessageKind::parse(&kind).ok_or_else(|| invalid("kind", &kind))?,
        text: row.get("text")?,
        card: card.map(|source| json_column("card", &source)).transpose()?,
        actions,
        agent_id,
        created_at: row.get("created_at")?,
    })
}

pub(super) fn to_repo(row: &Row<'_>) -> rusqlite::Result<RepoView> {
    let prefixes: String = row.get("ticket_prefixes")?;
    let notes: String = row.get("notes")?;
    Ok(RepoView {
        name: row.get("name")?,
        path: row.get("path")?,
        ticket_prefixes: json_column::<Vec<String>>("ticket_prefixes", &prefixes)?
            .iter()
            .map(|prefix| prefix.to_uppercase())
            .collect(),
        base_branch: row.get("base_branch")?,
        notes: json_column("notes", &notes)?,
        setup: row.get("setup")?,
    })
}

fn to_step_view(step: &StepRecord) -> AgentStepView {
    AgentStepView {
        id: step.id.clone(),
        brick_id: step.brick_id.clone(),
        name: step.name.clone(),
        state: step.state,
        pass: step.pass,
        model: step.model.clone(),
        started_at: step.started_at.clone(),
        ended_at: step.ended_at.clone(),
        wait_ms: step.wait_ms,
        usage: step.usage,
    }
}

/// The report of the last finished step that wrote one: a later one replaces it.
fn latest_report(steps: &[StepRecord]) -> Option<AgentReport> {
    steps.iter().rev().find_map(|step| {
        let written_at = step.ended_at.clone()?;
        let markdown = step.output.as_ref()?.get(REPORT_FIELD)?.clone();
        Some(AgentReport {
            step_name: step.name.clone(),
            markdown,
            written_at,
        })
    })
}

pub fn to_agent_view(agent: &AgentRecord) -> AgentView {
    AgentView {
        id: agent.id.clone(),
        ticket_ref: agent.ticket_ref.clone(),
        title: agent.title.clone(),
        repo_name: agent.repo_name.clone(),
        repo_path: agent.repo_path.clone(),
        worktree_path: agent.worktree_path.clone(),
        branch: agent.branch.clone(),
        extra_repos: agent
            .workspaces
            .iter()
            .map(|workspace| workspace.repo_name.clone())
            .collect(),
        step_id: agent.step_id.clone(),
        steps: agent.steps.iter().map(to_step_view).collect(),
        status: agent.status,
        status_reason: agent.status_reason.clone(),
        step_started_at: agent.step_started_at.clone(),
        waiting_since: agent.waiting_since.clone(),
        mr_url: agent.mr_url.clone(),
        live_session_id: agent.live_session_id.clone(),
        report: latest_report(&agent.steps),
        created_at: agent.created_at.clone(),
        updated_at: agent.updated_at.clone(),
    }
}
