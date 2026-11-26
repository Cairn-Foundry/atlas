// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Agent lifecycle: launching sessions, moving each agent through its own workflow, waiting for
//! the master, review passes and the end of life.
//!
//! The service is written for a single-threaded runtime: between two `.await` nothing else touches
//! the registry, so a sequence of reads and writes on an agent is atomic. The pseudo-terminals read
//! on threads of their own and hand what they print to the runtime through a channel.
//!
//! This file holds the shared state and what every part of the lifecycle needs; each submodule
//! adds the methods of one part to `AgentService`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use chrono::{DateTime, Utc};
use foundry_pty::{PtyEvent, TerminalMirror};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use crate::brick::Brick;
use crate::catalogue::{Catalogue, LiveCatalogue, Refresh, apply_change, check_change, describe_change};
use crate::clock::Clock;
use crate::config::AtlasConfig;
use crate::contract::{
    AgentStatus, CardStep, CatalogueChange, CatalogueDecision, ChatMessage, MessageAction, MessageCard, MessageKind,
    MessageRole, parse_repo_note,
};
use crate::error::{Error, Result, refuse};
use crate::language::tr;
use crate::notifier::Notifier;
use crate::projects::ProjectRegistry;
use crate::setup::SetupResult;
use crate::store::{AgentPatch, AgentRecord, MessageExtras, StepRecord, Store};
use crate::terminal::{TerminalLauncher, TerminalProcess, Utf8Chunks};
use crate::workflow::require_brick;

mod approvals;
mod hooks;
mod installs;
mod language;
mod launch;
mod sessions;
mod steps;
mod supervision;
#[cfg(test)]
mod tests;
mod workspaces;

pub use steps::CompleteStepInput;
#[cfg(test)]
pub use steps::FieldValue;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anomaly {
    Threshold {
        agent_id: String,
    },
    SessionDied {
        agent_id: String,
        exit_code: Option<i32>,
    },
    /// The session waits for input while the agent should be working: nothing will move it on.
    Idle {
        agent_id: String,
    },
}

/// A step whose brick is validated by the orchestrator, waiting for its decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    /// Tells this request from a later one of the same agent.
    pub id: u64,
    pub agent_id: String,
    pub step_id: String,
}

/// What wakes the orchestrator outside the master's messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wakeup {
    Anomaly(Anomaly),
    Approval(ApprovalRequest),
}

/// What the clients of the factory are told as it happens.
#[derive(Debug, Clone)]
pub enum Event {
    Agent(Box<AgentRecord>),
    /// The agent left the board.
    Archived(String),
    Message(Box<ChatMessage>),
    /// The orchestrator started or stopped thinking.
    Busy(bool),
    /// The master started a new conversation with the orchestrator.
    Reset,
    /// The factory changed language: the catalogue and every text to come are in the new one.
    Language,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    Output { agent_id: String, data: String },
    Exit { agent_id: String, code: Option<i32> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ApprovalDecision {
    Approved,
    Rejected(String),
}

pub struct ServiceDeps {
    pub store: Store,
    pub catalogue: LiveCatalogue,
    pub config: AtlasConfig,
    pub home: PathBuf,
    /// Base URL the agents' MCP client and hooks use to reach the daemon.
    pub daemon_url: String,
    /// Shared secret sent by the agents' MCP client and hooks.
    pub token: String,
    /// Executable whose `hook` command Claude Code runs for the hooks of an agent session.
    pub hook_executable: String,
    pub launcher: Box<dyn TerminalLauncher>,
    pub notifier: Box<dyn Notifier>,
    /// Where Claude Code keeps its transcripts, read for what a step consumed.
    pub claude_dir: PathBuf,
    /// Claude Code's state file, where a repository is marked trusted.
    pub claude_state_path: PathBuf,
    /// Environment the agent sessions inherit.
    pub environment: Vec<(String, String)>,
    pub now: Clock,
    /// Delay before closing a session whose step just ended, so the MCP reply reaches it.
    pub close_delay: Duration,
}

struct LiveState {
    /// Prompts Atlas typed into the session (the first one, the orchestrator's): their UserPromptSubmit is not the master answering.
    atlas_prompts: Vec<String>,
    mirror: TerminalMirror,
    decoder: Utf8Chunks,
    /// An idle session is reported once, until the agent shows signs of work again.
    is_idle_reported: bool,
}

struct LiveSession {
    /// Tells this session from an earlier one of the same agent when its terminal reports.
    token: u64,
    session_id: String,
    /// Session group and model the session was opened with: a later step may carry on in it.
    group: String,
    model: String,
    /// Whether the session was opened with the Agent tool: a later step that launches subagents needs it.
    has_subagents: bool,
    /// Skills and subagents the session was opened with: a later step that needs others opens its own.
    skills: Vec<String>,
    subagent_types: Vec<String>,
    /// The command line the session was started with, for whoever must understand why it misbehaves.
    command_line: String,
    process: Box<dyn TerminalProcess>,
    state: Mutex<LiveState>,
}

impl LiveSession {
    fn state(&self) -> MutexGuard<'_, LiveState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A dependency install Atlas runs in a worktree it created, while the agent starts its work.
#[derive(Clone)]
struct SetupRun {
    folder: String,
    command: String,
    log_path: String,
    /// None while the install runs.
    result: watch::Receiver<Option<SetupResult>>,
}

struct PendingApproval {
    id: u64,
    settle: oneshot::Sender<ApprovalDecision>,
}

type PtyReport = (String, u64, PtyEvent);

struct Inner {
    deps: ServiceDeps,
    projects: ProjectRegistry,
    live: Mutex<HashMap<String, Arc<LiveSession>>>,
    approvals: Mutex<HashMap<String, PendingApproval>>,
    setups: Mutex<HashMap<String, Vec<SetupRun>>>,
    events: broadcast::Sender<Event>,
    terminal: broadcast::Sender<TerminalEvent>,
    wakeups: Mutex<Option<mpsc::UnboundedSender<Wakeup>>>,
    pty_reports: mpsc::UnboundedSender<PtyReport>,
    next_token: AtomicU64,
}

pub const DEFAULT_CLOSE_DELAY: Duration = Duration::from_millis(1500);
const EVENT_CAPACITY: usize = 1024;
const TERMINAL_EVENT_CAPACITY: usize = 4096;

/// Runs blocking work (git, the file system) off the runtime thread.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| Error::internal(error.to_string()))
}

/// The agent factory. Cheap to clone: every clone drives the same agents.
#[derive(Clone)]
pub struct AgentService(Arc<Inner>);

impl AgentService {
    /// Builds the service; must be called from within the runtime that will run it.
    pub fn new(deps: ServiceDeps) -> Self {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let (terminal, _) = broadcast::channel(TERMINAL_EVENT_CAPACITY);
        let (pty_reports, mut reports) = mpsc::unbounded_channel::<PtyReport>();
        let projects = ProjectRegistry::new(deps.store.clone());
        let service = Self(Arc::new(Inner {
            deps,
            projects,
            live: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
            setups: Mutex::new(HashMap::new()),
            events,
            terminal,
            wakeups: Mutex::new(None),
            pty_reports,
            next_token: AtomicU64::new(1),
        }));
        let weak: Weak<Inner> = Arc::downgrade(&service.0);
        tokio::spawn(async move {
            while let Some((agent_id, token, event)) = reports.recv().await {
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                if let Err(error) = Self(inner).on_pty_event(&agent_id, token, event) {
                    tracing::error!(
                        "{}",
                        tr!(
                            "terminal de l'agent {agent_id} : {error}",
                            "terminal of the agent {agent_id}: {error}"
                        )
                    );
                }
            }
        });
        service
    }

    /// The catalogue in service, read again first if the master changed a brick file. A change that
    /// does not hold is refused, the master told why, and the catalogue he had stays.
    pub fn catalogue(&self) -> Arc<Catalogue> {
        match self.0.deps.catalogue.refresh() {
            Refresh::Unchanged => {}
            Refresh::Reloaded => {
                tracing::info!("{}", tr!("catalogue de briques relu", "catalogue of bricks read again"))
            }
            Refresh::Rejected(reason) => {
                let text = tr!(
                    "Les briques modifiées ne sont pas prises en compte, le catalogue précédent reste en service. {reason}",
                    "The changed bricks are not taken into account, the previous catalogue stays in service. {reason}"
                );
                if let Err(error) = self.warn_master(&text, None) {
                    tracing::error!(
                        "{}",
                        tr!(
                            "catalogue refusé ({reason}) : {error}",
                            "catalogue refused ({reason}): {error}"
                        )
                    );
                }
            }
        }
        self.0.deps.catalogue.current()
    }

    pub fn projects(&self) -> &ProjectRegistry {
        &self.0.projects
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.0.events.subscribe()
    }

    pub fn subscribe_terminal(&self) -> broadcast::Receiver<TerminalEvent> {
        self.0.terminal.subscribe()
    }

    /// Sends the anomalies and the steps waiting for a validation to whoever supervises: the orchestrator.
    pub fn wake(&self, wakeups: mpsc::UnboundedSender<Wakeup>) {
        *self.0.wakeups.lock().unwrap_or_else(PoisonError::into_inner) = Some(wakeups);
    }

    pub fn emit(&self, event: Event) {
        // Nobody listening is not an error: the registry holds the state a client reads when it connects.
        let _ = self.0.events.send(event);
    }

    fn send_wakeup(&self, wakeup: Wakeup) -> bool {
        self.0
            .wakeups
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|wakeups| wakeups.send(wakeup).is_ok())
    }

    fn store(&self) -> &Store {
        &self.0.deps.store
    }

    fn now(&self) -> DateTime<Utc> {
        (self.0.deps.now)()
    }

    fn live_sessions(&self) -> MutexGuard<'_, HashMap<String, Arc<LiveSession>>> {
        self.0.live.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn live(&self, agent_id: &str) -> Option<Arc<LiveSession>> {
        self.live_sessions().get(agent_id).cloned()
    }

    fn brick(&self, brick_id: &str) -> Result<Brick> {
        require_brick(&self.catalogue(), brick_id).cloned()
    }

    fn require_current<'a>(&self, agent: &'a AgentRecord) -> Result<&'a StepRecord> {
        match agent.current_step() {
            Some(step) => Ok(step),
            None => refuse!(
                "{} n'a aucune étape en cours",
                "{} has no current step",
                agent.ticket_ref
            ),
        }
    }

    fn require_not_delivered(&self, agent: &AgentRecord) -> Result<()> {
        if agent.status == AgentStatus::Done {
            refuse!("le travail est déjà livré", "the work is already delivered");
        }
        Ok(())
    }

    fn after(&self, delay: Duration, action: impl FnOnce(Self) + Send + 'static) {
        let service = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            action(service);
        });
    }

    fn update(&self, agent_id: &str, patch: AgentPatch) -> Result<AgentRecord> {
        let agent = self.store().update_agent(agent_id, patch)?;
        self.emit(Event::Agent(Box::new(agent.clone())));
        Ok(agent)
    }

    fn post_message(&self, text: &str, agent_id: Option<&str>, extras: MessageExtras) -> Result<ChatMessage> {
        let message = self
            .store()
            .add_message(MessageRole::Assistant, text, agent_id, extras)?;
        self.emit(Event::Message(Box::new(message.clone())));
        Ok(message)
    }

    /// An agent offers something it just learnt to the memory of its project. Nothing is kept until
    /// the master says so: the message carries the button that saves it.
    pub fn propose_note(
        &self,
        agent_id: &str,
        note: &str,
        repo_name: Option<&str>,
        is_for_all_projects: bool,
    ) -> Result<String> {
        let agent = self.store().require_agent(agent_id)?;
        let Ok(clean_note) = parse_repo_note(note) else {
            refuse!(
                "note refusée : une seule ligne, 300 caractères au plus",
                "note refused: a single line, 300 characters at most"
            );
        };
        let repo = if is_for_all_projects {
            None
        } else {
            let found = match repo_name.filter(|name| !name.is_empty()) {
                Some(name) => self.projects().find_by_reference(name)?,
                None => self.projects().find_by_path(&agent.repo_path)?,
            };
            let Some(found) = found else {
                return Ok(
                    "This repository is not a project declared in Atlas: nothing to keep, go on with your work."
                        .to_owned(),
                );
            };
            Some(found)
        };
        let known_notes = match &repo {
            Some(repo) => repo.notes.clone(),
            None => self.projects().list_global_notes()?,
        };
        if known_notes.contains(&clean_note) {
            return Ok("This note is already in memory. Go on with your work.".to_owned());
        }
        let scope = repo.as_ref().map_or_else(
            || tr!("pour tous les projets", "for all the projects"),
            |repo| tr!("pour le projet {}", "for the project {}", repo.name),
        );
        self.warn_master_with(
            &tr!(
                "L'agent qui travaille sur {} propose de retenir {scope} : {clean_note}",
                "The agent working on {} proposes to keep {scope}: {clean_note}",
                agent.ticket_ref
            ),
            Some(agent_id),
            vec![MessageAction::SaveNote {
                repo: repo.map(|repo| repo.name),
                note: clean_note,
            }],
        )?;
        Ok(
            "Proposal passed on to the master, who will decide whether to keep it. Go on with your work without waiting."
                .to_owned(),
        )
    }

    /// A launch, decision or workflow message: the reason, with the agent's steps as they now stand.
    fn post_card(&self, kind: MessageKind, agent: &AgentRecord, reason: &str) -> Result<()> {
        let card = MessageCard {
            ticket_ref: agent.ticket_ref.clone(),
            steps: agent
                .steps
                .iter()
                .map(|step| CardStep {
                    name: step.name.clone(),
                    state: step.state,
                })
                .collect(),
        };
        let extras = MessageExtras {
            kind: Some(kind),
            card: Some(card),
            actions: vec![MessageAction::Session],
        };
        self.post_message(reason, Some(&agent.id), extras)?;
        Ok(())
    }

    /// The orchestrator warns the master, it never relays the agent's words.
    pub fn warn_master(&self, text: &str, agent_id: Option<&str>) -> Result<ChatMessage> {
        let actions = if agent_id.is_some() {
            vec![MessageAction::Session]
        } else {
            Vec::new()
        };
        self.warn_master_with(text, agent_id, actions)
    }

    /// Shows the master a change to the catalogue the orchestrator proposes, once it holds: nothing
    /// is written before he validates it.
    pub fn propose_catalogue_change(&self, change: CatalogueChange, reason: &str) -> Result<String> {
        let reason = reason.trim();
        if reason.is_empty() {
            refuse!("reason : texte vide", "reason: empty text");
        }
        let deps = &self.0.deps;
        let catalogue = self.catalogue();
        check_change(&deps.home, &catalogue, &deps.config.default_steps, &change, false)?;
        let what = describe_change(&deps.home, &catalogue, &change);
        self.warn_master_with(
            &tr!(
                "L'orchestrateur propose de {what}. {reason}",
                "The orchestrator proposes to {what}. {reason}"
            ),
            None,
            vec![MessageAction::ApplyCatalogueChange { change, decision: None }],
        )?;
        Ok("Proposal shown to the master: nothing changes until he has validated it in the chat. Do not wait for his answer.".to_owned())
    }

    /// The master's decision on the change the message `message_id` proposes: applied, checked again
    /// against the catalogue as it is now, or refused. The message keeps the decision, so the card
    /// shows it from then on; a change is decided once. Only the change the message holds is
    /// applied, never one sent along with the decision.
    pub fn decide_catalogue_change(&self, message_id: &str, decision: CatalogueDecision) -> Result<ChatMessage> {
        let Some(message) = self.store().get_message(message_id)? else {
            refuse!("message inconnu : {message_id}", "unknown message: {message_id}");
        };
        let mut actions = message.actions;
        let Some(MessageAction::ApplyCatalogueChange {
            change,
            decision: pending,
        }) = actions
            .iter_mut()
            .find(|action| matches!(action, MessageAction::ApplyCatalogueChange { .. }))
        else {
            refuse!(
                "ce message ne propose aucun changement du catalogue",
                "this message proposes no change of the catalogue"
            );
        };
        if let Some(made) = pending {
            let made = match made {
                CatalogueDecision::Applied => tr!("appliqué", "applied"),
                CatalogueDecision::Refused => tr!("refusé", "refused"),
            };
            refuse!("ce changement est déjà {made}", "this change is already {made}");
        }
        if decision == CatalogueDecision::Applied {
            let deps = &self.0.deps;
            apply_change(&deps.home, &self.catalogue(), &deps.config.default_steps, change)?;
            // Read at once, so the next run of the orchestrator already describes it.
            self.catalogue();
        }
        *pending = Some(decision);
        let message = self.store().set_message_actions(message_id, &actions)?;
        self.emit(Event::Message(Box::new(message.clone())));
        Ok(message)
    }

    fn warn_master_with(&self, text: &str, agent_id: Option<&str>, actions: Vec<MessageAction>) -> Result<ChatMessage> {
        let message = self.post_message(
            text,
            agent_id,
            MessageExtras {
                actions,
                ..MessageExtras::default()
            },
        )?;
        self.0.deps.notifier.notify("Atlas", text);
        Ok(message)
    }
}
