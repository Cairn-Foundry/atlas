// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The public contract of the daemon: what its HTTP API and WebSockets carry.
//!
//! These types are the only description of the contract. The tests write them for the front as
//! `front/src/lib/contract.ts`, and one of everything the daemon sends as
//! `front/src/lib/contract-sample.ts`, which the front's compiler checks against those types.

use crate::language::tr;
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize};

pub use atlas_i18n::Language;
pub use atlas_link::DAEMON_NAME;

pub const DAEMON_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Lines of history the daemon keeps for a session's terminal.
pub const TERMINAL_SCROLLBACK_LINES: usize = 5000;

/// Field of `complete_step` that holds a rendering for the master, in Markdown.
pub const REPORT_FIELD: &str = "report";

const NOTE_MAX_CHARS: usize = 300;
const SETUP_MAX_CHARS: usize = 500;

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Working,
    Waiting,
    Blocked,
    Done,
}

impl AgentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Waiting => "waiting",
            Self::Blocked => "blocked",
            Self::Done => "done",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [Self::Working, Self::Waiting, Self::Blocked, Self::Done]
            .into_iter()
            .find(|status| status.as_str() == value)
    }
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepState {
    /// Planned, not started.
    Upcoming,
    Current,
    Done,
    /// Removed from the workflow before it started; kept so the board shows what was dropped.
    Skipped,
    /// Declared done outside Atlas when the agent was launched further down its workflow.
    External,
}

impl StepState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Upcoming => "upcoming",
            Self::Current => "current",
            Self::Done => "done",
            Self::Skipped => "skipped",
            Self::External => "external",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [Self::Upcoming, Self::Current, Self::Done, Self::Skipped, Self::External]
            .into_iter()
            .find(|state| state.as_str() == value)
    }
}

/// Tokens a step consumed, read from the Claude Code transcripts of its sessions.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

/// One step of an agent's own workflow: a brick of the catalogue, with what it cost.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStepView {
    pub id: String,
    pub brick_id: String,
    pub name: String,
    pub state: StepState,
    /// Review pass, 1 for anything that is not a repeated review.
    pub pass: u32,
    pub model: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    /// Milliseconds spent waiting for the master during the step.
    pub wait_ms: i64,
    pub usage: Option<StepUsage>,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentReport {
    /// Name of the step that wrote it.
    pub step_name: String,
    pub markdown: String,
    pub written_at: String,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    pub id: String,
    pub ticket_ref: String,
    pub title: String,
    /// Name of the project the agent works in: the declared one, else the repository folder.
    pub repo_name: String,
    pub repo_path: String,
    pub worktree_path: String,
    pub branch: String,
    /// Names of the other projects the agent also works in, each in a worktree on the same branch.
    pub extra_repos: Vec<String>,
    /// Brick of the current step.
    pub step_id: String,
    /// The agent's workflow, in order: what it went through, where it is, what remains.
    pub steps: Vec<AgentStepView>,
    pub status: AgentStatus,
    pub status_reason: Option<String>,
    /// ISO date the agent entered its current step.
    pub step_started_at: String,
    /// ISO date the agent started waiting, none when not waiting.
    pub waiting_since: Option<String>,
    pub mr_url: Option<String>,
    /// Session currently attached to the agent's terminal, none when closed.
    pub live_session_id: Option<String>,
    /// Latest rendering written for the master, shown without reopening the session.
    pub report: Option<AgentReport>,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub id: String,
    pub agent_id: String,
    /// Session group of the brick the session was opened for.
    pub group: String,
    pub model: String,
    /// Brick the session was opened for.
    pub step_id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub outcome: Option<String>,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageKind {
    Text,
    /// The orchestrator launched an agent with this workflow.
    Launch,
    /// The orchestrator validated or sent back a step.
    Decision,
    /// The upcoming steps of an agent changed.
    Workflow,
}

impl MessageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Launch => "launch",
            Self::Decision => "decision",
            Self::Workflow => "workflow",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [Self::Text, Self::Launch, Self::Decision, Self::Workflow]
            .into_iter()
            .find(|kind| kind.as_str() == value)
    }
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardStep {
    pub name: String,
    pub state: StepState,
}

/// What a launch, decision or workflow message shows besides its reason.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageCard {
    pub ticket_ref: String,
    pub steps: Vec<CardStep>,
}

/// A change to the master's catalogue in `~/.atlas`, proposed by the orchestrator and applied only
/// once the master validates it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum CatalogueChange {
    /// Writes `bricks/<id>.yml`: a new brick, or a new version of one.
    Brick { id: String, content: String },
    /// Writes the folder `skills/<name>/`, whole: each file by its path in the folder.
    Skill {
        name: String,
        files: IndexMap<String, String>,
    },
    /// Writes `subagents/<name>.md`.
    Subagent { name: String, content: String },
    /// Takes a brick out of service; a brick is never deleted.
    Disable { id: String },
    /// Puts a disabled brick back in service.
    Enable { id: String },
}

/// What the master made of a change to the catalogue the orchestrator proposed.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CatalogueDecision {
    Applied,
    Refused,
}

/// Body of `POST /api/messages/:id/catalogue-change`: the master's decision on the change the message proposes.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogueDecisionInput {
    pub decision: CatalogueDecision,
}

/// What the master can do straight from a message: the buttons under it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum MessageAction {
    /// Open the agent's session view.
    Session,
    /// Open a page elsewhere, such as the merge request.
    Link { label: String, url: String },
    /// Reopen the agent's closed session.
    Resume,
    /// Take the agent off the board.
    Archive,
    /// Keep `note` in the memory of the project `repo`, or of every project when `repo` is none.
    SaveNote { repo: Option<String>, note: String },
    /// Apply or refuse a change to the catalogue the orchestrator proposed; `decision` is set once
    /// the master made it, and the message keeps it.
    ApplyCatalogueChange {
        change: CatalogueChange,
        #[serde(default)]
        decision: Option<CatalogueDecision>,
    },
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

impl MessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub role: MessageRole,
    pub kind: MessageKind,
    /// The message itself; for a card, the reason the orchestrator gave.
    pub text: String,
    pub card: Option<MessageCard>,
    /// Buttons to offer under the message, in order; they act on `agent_id`.
    pub actions: Vec<MessageAction>,
    pub agent_id: Option<String>,
    pub created_at: String,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrickSummary {
    pub id: String,
    pub name: String,
}

#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// The language the daemon speaks, which the page speaks too.
    pub language: Language,
    /// The catalogue, for the names of the steps.
    pub bricks: Vec<BrickSummary>,
    /// Agents on the board: archived ones are left out.
    pub agents: Vec<AgentView>,
    /// Messages of the current conversation, since the last reset.
    pub messages: Vec<ChatMessage>,
    /// True while the orchestrator is thinking, so a client that connects mid-run shows it.
    pub orchestrator_busy: bool,
}

/// Body of `POST /api/language`: the language the master picks for the whole factory.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct LanguageInput {
    pub language: Language,
}

/// Messages pushed on `GET /ws`.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ServerEvent {
    Snapshot {
        snapshot: Box<Snapshot>,
    },
    Agent {
        agent: Box<AgentView>,
    },
    /// The agent left the board.
    Archived {
        #[serde(rename = "agentId")]
        agent_id: String,
    },
    Message {
        message: Box<ChatMessage>,
    },
    Orchestrator {
        busy: bool,
    },
    /// The master started a new conversation: the client empties the chat thread.
    Reset,
}

/// Messages pushed on `GET /ws/terminal/:agentId`.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TerminalServerEvent {
    Output { data: String },
    Exit { code: Option<i32> },
}

/// Messages the terminal of the page sends on `GET /ws/terminal/:agentId`.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TerminalClientEvent {
    Input { data: String },
    Resize { cols: u16, rows: u16 },
}

/// Answer of `GET /api/health`.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize)]
pub struct DaemonHealth {
    pub name: &'static str,
    pub version: &'static str,
}

/// A project Atlas can launch agents in.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoView {
    pub name: String,
    pub path: String,
    /// Ticket prefixes that belong to this project: `CAIRN` sends `CAIRN-123` here.
    pub ticket_prefixes: Vec<String>,
    /// Branch new work starts from, fetched from origin first; none means the branch checked out.
    pub base_branch: Option<String>,
    /// The project's memory: what the master taught an agent once and every later agent should know.
    pub notes: Vec<String>,
    /// Shell command that installs the dependencies, run in each worktree Atlas creates.
    pub setup: Option<String>,
}

/// A field that tells "absent" (outer none) from "null" (inner none).
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// One line of at most `max_chars` characters, trimmed; the reason when it is not.
fn single_line(value: &str, max_chars: usize) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(tr!("texte vide", "empty text"));
    }
    if trimmed.chars().count() > max_chars {
        return Err(tr!("{max_chars} caractères au plus", "{max_chars} characters at most"));
    }
    if trimmed.contains('\n') {
        return Err(tr!("une seule ligne", "a single line"));
    }
    Ok(trimmed.to_owned())
}

/// One line of a project's memory.
pub fn parse_repo_note(note: &str) -> Result<String, String> {
    single_line(note, NOTE_MAX_CHARS)
}

/// The command that installs a project's dependencies.
pub fn parse_repo_setup(setup: &str) -> Result<String, String> {
    single_line(setup, SETUP_MAX_CHARS)
}

pub fn is_brick_id(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-')
}

fn is_ticket_prefix(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// One problem of an input, with the field it is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub path: String,
    pub message: String,
}

impl Issue {
    pub fn new(path: &str, message: impl Into<String>) -> Self {
        Self {
            path: path.to_owned(),
            message: message.into(),
        }
    }
}

/// Issues on one line, for a refusal read by an agent or the orchestrator.
pub fn inline_issues(issues: &[Issue], is_path_shown: bool) -> String {
    issues
        .iter()
        .map(|issue| {
            if is_path_shown {
                format!("{} : {}", issue.path, issue.message)
            } else {
                issue.message.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ; ")
}

/// A project as the master or the orchestrator declares it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoInput {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub ticket_prefixes: Vec<String>,
    #[serde(default)]
    pub base_branch: Option<String>,
    /// Notes to add to the project's memory; the ones it already holds are kept.
    #[serde(default)]
    pub notes: Vec<String>,
    /// Command that installs the dependencies in a new worktree; absent keeps the current one, null removes it.
    #[serde(default, deserialize_with = "present")]
    pub setup: Option<Option<String>>,
}

impl RepoInput {
    /// The input trimmed and checked, or what is wrong with it.
    pub fn validated(mut self) -> Result<Self, Vec<Issue>> {
        let mut issues = Vec::new();
        self.name = self.name.trim().to_owned();
        self.path = self.path.trim().to_owned();
        let is_name_valid = !self.name.is_empty()
            && self
                .name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'));
        if !is_name_valid {
            issues.push(Issue::new(
                "name",
                &tr!(
                    "nom : lettres, chiffres, point, tiret, souligné",
                    "name: letters, digits, dot, dash, underscore"
                ),
            ));
        }
        if self.path.is_empty() {
            issues.push(Issue::new("path", &tr!("chemin vide", "empty path")));
        }
        self.ticket_prefixes = self
            .ticket_prefixes
            .iter()
            .map(|prefix| prefix.trim().to_owned())
            .collect();
        if !self.ticket_prefixes.iter().all(|prefix| is_ticket_prefix(prefix)) {
            issues.push(Issue::new(
                "ticketPrefixes",
                &tr!(
                    "préfixe : une lettre puis lettres, chiffres ou souligné",
                    "prefix: a letter then letters, digits or underscore"
                ),
            ));
        }
        self.base_branch = self.base_branch.map(|branch| branch.trim().to_owned());
        if self.base_branch.as_deref() == Some("") {
            issues.push(Issue::new("baseBranch", &tr!("branche vide", "empty branch")));
        }
        let mut notes = Vec::new();
        for note in &self.notes {
            match parse_repo_note(note) {
                Ok(note) => notes.push(note),
                Err(reason) => issues.push(Issue::new("notes", reason)),
            }
        }
        self.notes = notes;
        if let Some(Some(setup)) = &self.setup {
            match parse_repo_setup(setup) {
                Ok(parsed) => self.setup = Some(Some(parsed)),
                Err(reason) => issues.push(Issue::new("setup", reason)),
            }
        }
        if issues.is_empty() { Ok(self) } else { Err(issues) }
    }
}

/// What launches an agent. The project is found from `repo_path`, else from the project `repo`
/// name, else from the prefix of `ticket_ref` (CAIRN-123 goes to the project declaring CAIRN).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgentInput {
    pub ticket_ref: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub repo_path: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    /// The agent's workflow, as brick ids in order; absent means the default workflow.
    #[serde(default)]
    pub steps: Option<Vec<String>>,
    /// Brick the agent starts at; the ones before it are recorded as done outside Atlas.
    #[serde(default)]
    pub start_at: Option<String>,
    /// Why this workflow, shown in the chat.
    #[serde(default)]
    pub reason: Option<String>,
    /// Existing branch to work on, local or remote; absent means a new branch.
    #[serde(default)]
    pub branch: Option<String>,
    /// Existing worktree to work in.
    #[serde(default)]
    pub worktree_path: Option<String>,
    /// Work in the main folder of the project, without a worktree.
    #[serde(default)]
    pub in_place: Option<bool>,
    /// New branch: its type (feat, fix...) and its short name.
    #[serde(default)]
    pub branch_type: Option<String>,
    #[serde(default)]
    pub branch_name: Option<String>,
    /// Model of the free task brick.
    #[serde(default)]
    pub task_model: Option<String>,
    /// Merge request the work is about, for a review of an existing MR.
    #[serde(default)]
    pub mr_url: Option<String>,
}

fn is_url(value: &str) -> bool {
    value.split_once("://").is_some_and(|(scheme, rest)| {
        !rest.is_empty()
            && !value.contains(char::is_whitespace)
            && scheme.chars().next().is_some_and(|first| first.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.'))
    })
}

impl CreateAgentInput {
    /// The input trimmed and checked, or what is wrong with it.
    pub fn validated(mut self) -> Result<Self, Vec<Issue>> {
        let mut issues = Vec::new();
        let mut non_empty = |path: &str, value: &Option<String>| {
            if value.as_deref() == Some("") {
                issues.push(Issue::new(path, &tr!("texte vide", "empty text")));
            }
        };
        let trimmed = |value: Option<String>| value.map(|text| text.trim().to_owned());
        self.reason = trimmed(self.reason);
        self.branch = trimmed(self.branch);
        self.worktree_path = trimmed(self.worktree_path);
        self.branch_type = trimmed(self.branch_type);
        self.branch_name = trimmed(self.branch_name);
        self.task_model = trimmed(self.task_model);
        self.ticket_ref = self.ticket_ref.trim().to_owned();
        non_empty("ticketRef", &Some(self.ticket_ref.clone()));
        non_empty("title", &self.title);
        non_empty("repo", &self.repo);
        non_empty("repoPath", &self.repo_path);
        non_empty("reason", &self.reason);
        non_empty("branch", &self.branch);
        non_empty("worktreePath", &self.worktree_path);
        non_empty("branchName", &self.branch_name);
        non_empty("taskModel", &self.task_model);
        if let Some(steps) = &self.steps {
            if steps.is_empty() {
                issues.push(Issue::new("steps", &tr!("au moins une brique", "at least one brick")));
            }
            if !steps.iter().all(|step| is_brick_id(step)) {
                issues.push(Issue::new(
                    "steps",
                    &tr!("identifiant de brique invalide", "invalid brick id"),
                ));
            }
        }
        if self.start_at.as_deref().is_some_and(|start| !is_brick_id(start)) {
            issues.push(Issue::new(
                "startAt",
                &tr!("identifiant de brique invalide", "invalid brick id"),
            ));
        }
        if self
            .branch_type
            .as_deref()
            .is_some_and(|kind| kind.is_empty() || !kind.chars().all(|c| c.is_ascii_lowercase()))
        {
            issues.push(Issue::new(
                "branchType",
                &tr!("lettres minuscules seulement", "lowercase letters only"),
            ));
        }
        if self.mr_url.as_deref().is_some_and(|url| !is_url(url)) {
            issues.push(Issue::new("mrUrl", &tr!("lien invalide", "invalid link")));
        }
        if issues.is_empty() { Ok(self) } else { Err(issues) }
    }
}

/// What a client posts on `POST /api/messages`.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PostMessageInput {
    pub text: String,
    #[serde(default)]
    pub broadcast: bool,
    /// Ids of the agents mentioned with @ in the message; the orchestrator gets their context.
    #[serde(default)]
    pub mentions: Vec<String>,
}

const MAX_MENTIONS: usize = 20;

impl PostMessageInput {
    pub fn validated(mut self) -> Result<Self, Vec<Issue>> {
        self.text = self.text.trim().to_owned();
        let mut issues = Vec::new();
        if self.text.is_empty() {
            issues.push(Issue::new("text", &tr!("texte vide", "empty text")));
        }
        if self.mentions.len() > MAX_MENTIONS || self.mentions.iter().any(String::is_empty) {
            issues.push(Issue::new(
                "mentions",
                &tr!(
                    "vingt agents au plus, identifiants non vides",
                    "twenty agents at most, non-empty ids"
                ),
            ));
        }
        if issues.is_empty() { Ok(self) } else { Err(issues) }
    }
}

/// The fields a finished step handed over: its section of the agent's file, in the order the brick asks for them.
pub type StepOutput = IndexMap<String, String>;

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn serializes_events_and_actions_as_the_front_reads_them() {
        let action = MessageAction::SaveNote {
            repo: None,
            note: "host".into(),
        };
        assert_eq!(
            serde_json::to_value(&action).expect("json"),
            json!({ "type": "save-note", "repo": null, "note": "host" })
        );
        assert_eq!(
            serde_json::to_value(MessageAction::Session).expect("json"),
            json!({ "type": "session" })
        );
        let archived = ServerEvent::Archived { agent_id: "a1".into() };
        assert_eq!(
            serde_json::to_value(&archived).expect("json"),
            json!({ "type": "archived", "agentId": "a1" })
        );
        assert_eq!(
            serde_json::to_value(ServerEvent::Reset).expect("json"),
            json!({ "type": "reset" })
        );
        let exit = TerminalServerEvent::Exit { code: None };
        assert_eq!(
            serde_json::to_value(&exit).expect("json"),
            json!({ "type": "exit", "code": null })
        );
    }

    #[test]
    fn reads_terminal_client_events_and_refuses_a_zero_size() {
        let resize: TerminalClientEvent =
            serde_json::from_value(json!({ "type": "resize", "cols": 120, "rows": 40 })).expect("resize");
        assert_eq!(resize, TerminalClientEvent::Resize { cols: 120, rows: 40 });
        assert!(
            serde_json::from_value::<TerminalClientEvent>(json!({ "type": "resize", "cols": -1, "rows": 40 })).is_err()
        );
    }

    #[test]
    fn tells_an_absent_setup_from_a_removed_one() {
        let absent: RepoInput = serde_json::from_value(json!({ "name": "atlas", "path": "/repo" })).expect("absent");
        assert_eq!(absent.setup, None);
        let removed: RepoInput =
            serde_json::from_value(json!({ "name": "atlas", "path": "/repo", "setup": null })).expect("removed");
        assert_eq!(removed.setup, Some(None));
    }

    #[test]
    fn validates_a_project_and_names_each_problem() {
        let input = RepoInput {
            name: "my repo".into(),
            path: " ".into(),
            ticket_prefixes: vec!["1X".into()],
            ..RepoInput::default()
        };
        let issues = input.validated().expect_err("issues");
        assert_eq!(
            issues.iter().map(|issue| issue.path.as_str()).collect::<Vec<_>>(),
            ["name", "path", "ticketPrefixes"]
        );
    }

    #[test]
    fn keeps_a_note_on_one_short_line() {
        assert_eq!(
            parse_repo_note("  host is api.test  "),
            Ok("host is api.test".to_owned())
        );
        assert!(parse_repo_note("two\nlines").is_err());
        assert!(parse_repo_note(&"x".repeat(301)).is_err());
        assert!(parse_repo_note("   ").is_err());
    }

    #[test]
    fn validates_a_launch() {
        let valid: CreateAgentInput = serde_json::from_value(
            json!({ "ticketRef": "SHOP-1", "steps": ["plan"], "mrUrl": "https://forge.test/mr/1" }),
        )
        .expect("input");
        assert!(valid.validated().is_ok());
        let invalid: CreateAgentInput = serde_json::from_value(
            json!({ "ticketRef": "", "steps": ["Plan"], "startAt": "9x", "branchType": "Feat", "mrUrl": "not a link" }),
        )
        .expect("input");
        let issues = invalid.validated().expect_err("issues");
        assert_eq!(inline_issues(&issues, true).matches(" : ").count(), 5);
        let blank: CreateAgentInput = serde_json::from_value(json!({ "ticketRef": "  " })).expect("input");
        assert_eq!(blank.validated().expect_err("issues")[0].path, "ticketRef");
    }

    /// One of everything the daemon sends, written as a TypeScript value the front's compiler
    /// checks against the generated types: what serde writes and what the types say cannot differ
    /// without `pnpm typecheck` failing. `ATLAS_UPDATE_CONTRACT_SAMPLE=1 cargo test contract_sample`
    /// rewrites it.
    #[test]
    fn contract_sample_is_what_the_front_compiles_against() {
        use crate::fixtures::review_agent;
        use crate::store::to_agent_view;

        let mut record = review_agent();
        record.steps[0].started_at = Some("2026-01-01T09:00:00.000Z".into());
        record.steps[0].usage = Some(StepUsage {
            input_tokens: 1200,
            output_tokens: 340,
            cache_read_tokens: 5600,
            cache_write_tokens: 78,
        });
        record.steps[0]
            .output
            .get_or_insert_default()
            .insert(REPORT_FIELD.to_owned(), "# Rapport".to_owned());
        let agent = to_agent_view(&record);
        let message = ChatMessage {
            id: "message-1".into(),
            role: MessageRole::Assistant,
            kind: MessageKind::Launch,
            text: "Lancé avec le parcours par défaut.".into(),
            card: Some(MessageCard {
                ticket_ref: agent.ticket_ref.clone(),
                steps: vec![CardStep {
                    name: "Review".into(),
                    state: StepState::Current,
                }],
            }),
            actions: vec![
                MessageAction::Session,
                MessageAction::Link {
                    label: "Ouvrir la MR".into(),
                    url: "https://forge.test/mr/4".into(),
                },
                MessageAction::Resume,
                MessageAction::Archive,
                MessageAction::SaveNote {
                    repo: Some("shop".into()),
                    note: "host".into(),
                },
                MessageAction::SaveNote {
                    repo: None,
                    note: "host".into(),
                },
            ],
            agent_id: Some(agent.id.clone()),
            created_at: "2026-01-01T10:00:00.000Z".into(),
        };
        let plain = ChatMessage {
            id: "message-2".into(),
            role: MessageRole::User,
            kind: MessageKind::Text,
            text: "Bonjour".into(),
            card: None,
            actions: Vec::new(),
            agent_id: None,
            created_at: "2026-01-01T10:01:00.000Z".into(),
        };
        let snapshot = Snapshot {
            language: Language::Fr,
            bricks: vec![BrickSummary {
                id: "review".into(),
                name: "Review".into(),
            }],
            agents: vec![agent.clone()],
            messages: vec![message.clone(), plain],
            orchestrator_busy: true,
        };
        let sample = json!({
            "health": DaemonHealth { name: DAEMON_NAME, version: "0.1.0" },
            "repos": [
                RepoView {
                    name: "shop".into(),
                    path: "/repos/shop".into(),
                    ticket_prefixes: vec!["SHOP".into()],
                    base_branch: Some("develop".into()),
                    notes: vec!["host".into()],
                    setup: Some("pnpm install".into()),
                },
                RepoView {
                    name: "libre".into(),
                    path: "/repos/libre".into(),
                    ticket_prefixes: Vec::new(),
                    base_branch: None,
                    notes: Vec::new(),
                    setup: None,
                },
            ],
            "sessions": [
                SessionView {
                    id: "session-1".into(),
                    agent_id: agent.id.clone(),
                    group: "review".into(),
                    model: "opus".into(),
                    step_id: "review".into(),
                    started_at: "2026-01-01T09:00:00.000Z".into(),
                    ended_at: Some("2026-01-01T09:30:00.000Z".into()),
                    outcome: Some("exited".into()),
                },
                SessionView {
                    id: "session-2".into(),
                    agent_id: agent.id.clone(),
                    group: "review".into(),
                    model: "sonnet".into(),
                    step_id: "review".into(),
                    started_at: "2026-01-01T09:31:00.000Z".into(),
                    ended_at: None,
                    outcome: None,
                },
            ],
            "serverEvents": [
                ServerEvent::Snapshot { snapshot: Box::new(snapshot) },
                ServerEvent::Agent { agent: Box::new(agent.clone()) },
                ServerEvent::Archived { agent_id: agent.id.clone() },
                ServerEvent::Message { message: Box::new(message) },
                ServerEvent::Orchestrator { busy: false },
                ServerEvent::Reset,
            ],
            "terminalEvents": [
                TerminalServerEvent::Output { data: "bonjour".into() },
                TerminalServerEvent::Exit { code: Some(0) },
                TerminalServerEvent::Exit { code: None },
            ],
        });
        let generated = format!(
            "// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors\n\
             // SPDX-License-Identifier: AGPL-3.0-or-later\n\n\
             // Generated by the daemon's tests from what it serializes: do not edit.\n\
             // To rewrite it: ATLAS_UPDATE_CONTRACT_SAMPLE=1 cargo test -p atlas-daemon contract_sample\n\
             import type {{ DaemonHealth, RepoView, ServerEvent, SessionView, TerminalServerEvent }} from './contract';\n\n\
             /** One of everything the daemon sends, checked here against the types the front reads. */\n\
             export const contractSample = {} satisfies {{\n  health: DaemonHealth;\n  repos: RepoView[];\n  sessions: SessionView[];\n  serverEvents: ServerEvent[];\n  terminalEvents: TerminalServerEvent[];\n}};\n",
            serde_json::to_string_pretty(&sample).expect("json")
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../front/src/lib/contract-sample.ts");
        if std::env::var_os("ATLAS_UPDATE_CONTRACT_SAMPLE").is_some() {
            std::fs::write(&path, &generated).expect("write");
        }
        let published = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            published == generated,
            "{} is not what the daemon serializes: rerun with ATLAS_UPDATE_CONTRACT_SAMPLE=1",
            path.display()
        );
    }

    fn declare<T: ts_rs::TS>(config: &ts_rs::Config) -> String {
        format!("{}export {}", T::docs().unwrap_or_default(), T::decl(config))
    }

    /// The types of everything the daemon sends and receives, written for the front from the
    /// Rust ones: nobody copies a field by hand. `ATLAS_UPDATE_CONTRACT=1 cargo test contract_types`
    /// rewrites the file.
    #[test]
    fn contract_types_are_the_ones_the_front_compiles_against() {
        let config = ts_rs::Config::new().with_large_int("number");
        let declarations = [
            declare::<Language>(&config),
            declare::<LanguageInput>(&config),
            declare::<AgentStatus>(&config),
            declare::<StepState>(&config),
            declare::<StepUsage>(&config),
            declare::<AgentStepView>(&config),
            declare::<AgentReport>(&config),
            declare::<AgentView>(&config),
            declare::<SessionView>(&config),
            declare::<MessageKind>(&config),
            declare::<CardStep>(&config),
            declare::<MessageCard>(&config),
            declare::<CatalogueChange>(&config),
            declare::<CatalogueDecision>(&config),
            declare::<CatalogueDecisionInput>(&config),
            declare::<MessageAction>(&config),
            declare::<MessageRole>(&config),
            declare::<ChatMessage>(&config),
            declare::<BrickSummary>(&config),
            declare::<Snapshot>(&config),
            declare::<ServerEvent>(&config),
            declare::<TerminalServerEvent>(&config),
            declare::<TerminalClientEvent>(&config),
            declare::<DaemonHealth>(&config),
            declare::<RepoView>(&config),
            declare::<PostMessageInput>(&config),
        ];
        let generated = format!(
            "// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors\n\
             // SPDX-License-Identifier: AGPL-3.0-or-later\n\n\
             // Generated by the daemon's tests from its Rust types (contract.rs): do not edit.\n\
             // To rewrite it: ATLAS_UPDATE_CONTRACT=1 cargo test -p atlas-daemon contract_types\n\n\
             {}\n\n\
             /** Lines of history the daemon keeps for a session's terminal. */\n\
             export const TERMINAL_SCROLLBACK_LINES = {TERMINAL_SCROLLBACK_LINES};\n",
            declarations.join("\n\n")
        );
        assert_published("../../front/src/lib/contract.ts", &generated, "ATLAS_UPDATE_CONTRACT");
    }

    /// Compares a generated file with the one in the repository, rewriting it when `update` is set.
    fn assert_published(relative_path: &str, generated: &str, update: &str) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path);
        if std::env::var_os(update).is_some() {
            std::fs::write(&path, generated).expect("write");
        }
        let published = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            published == generated,
            "{} is not what the daemon generates: rerun the tests with {update}=1",
            path.display()
        );
    }
}
