// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The LLM half of the orchestrator: woken by the master's chat messages, by anomalies and by
//! steps waiting for its validation. Chat and anomaly runs go one at a time so the chat session is
//! never resumed twice concurrently; a validation is a run of its own, since an agent waits on it.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::catalogue::{Catalogue, LiveCatalogue};
use crate::claude::{claude_env, process_env, write_atlas_mcp_config};
use crate::clock::{Clock, iso};
use crate::config::AtlasConfig;
use crate::contract::{ChatMessage, MessageRole, StepUsage};
use crate::error::{Error, Result, refuse};
use crate::files::write_whole;
use crate::language::{self, Language, tr};
use crate::orchestrator_prompt::build_orchestrator_prompt;
use crate::service::{AgentService, Anomaly, ApprovalRequest, Event, Wakeup};
use crate::store::{AgentRecord, MessageExtras, OrchestratorRun, OrchestratorRunKind, Store};
use crate::workflow::{active_step_ms, require_brick};

const ORCHESTRATOR_SESSION_KEY: &str = "orchestrator.chatSessionId";
const CONTEXT_TOKENS_KEY: &str = "orchestrator.chatContextTokens";
const CLEARED_AT_KEY: &str = "orchestrator.clearedAt";
/// Messages replayed to a fresh chat session after a rotation, so the thread keeps its footing.
const ROTATION_RECAP_MESSAGES: usize = 6;
const MS_PER_MINUTE: f64 = 60_000.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeRun {
    pub prompt: String,
    pub resume_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaudeResult {
    pub text: String,
    pub session_id: Option<String>,
    pub is_error: bool,
    /// Tokens of the whole run, every turn added up; absent when the runner cannot tell.
    pub usage: Option<StepUsage>,
    pub turns: Option<u32>,
    pub duration_ms: Option<u64>,
}

type RunFuture = Pin<Box<dyn Future<Output = std::result::Result<ClaudeResult, String>> + Send>>;

/// Runs the orchestrator model on one prompt; the error is why the run could not be read.
pub type ClaudeRunner = Arc<dyn Fn(ClaudeRun) -> RunFuture + Send + Sync>;

#[derive(Debug, Default, Deserialize)]
struct RawUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawResult {
    result: Option<String>,
    session_id: Option<String>,
    is_error: Option<bool>,
    num_turns: Option<u32>,
    duration_ms: Option<u64>,
    usage: Option<RawUsage>,
}

/// What `claude -p --output-format json` printed, as a result; none when it is not one.
fn parse_claude_output(stdout: &str, has_succeeded: bool) -> Option<ClaudeResult> {
    let raw: RawResult = serde_json::from_str(stdout).ok()?;
    Some(ClaudeResult {
        text: raw.result.unwrap_or_default(),
        session_id: raw.session_id,
        is_error: raw.is_error.unwrap_or(!has_succeeded),
        usage: raw.usage.map(|usage| StepUsage {
            input_tokens: usage.input_tokens.unwrap_or(0),
            output_tokens: usage.output_tokens.unwrap_or(0),
            cache_read_tokens: usage.cache_read_input_tokens.unwrap_or(0),
            cache_write_tokens: usage.cache_creation_input_tokens.unwrap_or(0),
        }),
        turns: raw.num_turns,
        duration_ms: raw.duration_ms,
    })
}

struct RunnerFiles {
    dir: PathBuf,
    mcp_config: String,
    system_prompt: String,
}

async fn run_claude(
    command: &str,
    model: &str,
    files: &RunnerFiles,
    run: ClaudeRun,
) -> std::result::Result<ClaudeResult, String> {
    let environment = claude_env(&process_env(), &[]);
    let resolved = foundry_pty::resolve_command(
        command,
        environment.iter().map(|(key, value)| (key.as_str(), value.as_str())),
    );
    // The prompt goes through stdin and the system prompt through a file: no free text in the
    // command line, so no quoting issue even when Windows runs `claude` through cmd.exe.
    let mut child = Command::new(&resolved.file)
        .args(&resolved.prefix)
        .args([
            "-p",
            "--output-format",
            "json",
            "--model",
            model,
            "--mcp-config",
            &files.mcp_config,
        ])
        .args(["--system-prompt-file", &files.system_prompt, "--disable-slash-commands"])
        // Built-in tools to explore the machine for context, and the search that loads the MCP tools,
        // Atlas's and the master's (the forge). No prompt can be answered in a headless run: in auto
        // mode a classifier decides instead, and turns down what the master did not ask for.
        .args([
            "--tools",
            "ToolSearch,Read,Glob,Grep,Bash,WebFetch",
            "--permission-mode",
            "auto",
        ])
        .args(
            run.resume_id
                .iter()
                .flat_map(|resume_id| ["--resume", resume_id.as_str()]),
        )
        .current_dir(&files.dir)
        .env_clear()
        .envs(environment)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            tr!(
                "lancement de {command} impossible : {error}",
                "{command} could not be started: {error}"
            )
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        // A process that exits before reading its prompt reports why in its own output.
        let _ = stdin.write_all(run.prompt.as_bytes()).await;
    }
    let output = child.wait_with_output().await.map_err(|error| error.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_claude_output(&stdout, output.status.success()).ok_or_else(|| {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        let code = output
            .status
            .code()
            .map_or_else(|| tr!("inconnu", "unknown"), |code| code.to_string());
        tr!(
            "claude -p a échoué (code {code}) : {detail}",
            "claude -p failed (code {code}): {detail}"
        )
    })
}

/// Runs `claude -p` on the master's subscription, no API key. The session is a light one:
/// Atlas's own system prompt and no built-in tool but the tool search, which keeps the master's
/// MCP servers reachable (to read a ticket) without loading their schemas on every run.
pub fn create_claude_runner(
    config: &AtlasConfig,
    home: &Path,
    daemon_url: &str,
    token: &str,
    catalogue: &LiveCatalogue,
) -> Result<ClaudeRunner> {
    let dir = home.join("orchestrator");
    std::fs::create_dir_all(&dir)?;
    let mcp_config = dir.join("mcp.json");
    let system_prompt = dir.join("system-prompt.md");
    let described = catalogue.current();
    std::fs::write(&system_prompt, build_orchestrator_prompt(described.values()))?;
    // The catalogue the prompt file describes and the language it tells the orchestrator to
    // speak: the file follows the bricks the master edits and the language he picks.
    let described = Arc::new(Mutex::new((described, language::current())));
    let catalogue = catalogue.clone();
    write_atlas_mcp_config(&mcp_config, &format!("{daemon_url}/mcp/orchestrator"), token)?;
    let files = Arc::new(RunnerFiles {
        dir,
        mcp_config: mcp_config.to_string_lossy().into_owned(),
        system_prompt: system_prompt.to_string_lossy().into_owned(),
    });
    let command = config.claude_command.clone();
    let model = config.orchestrator_model.clone();
    let timeout = Duration::from_secs(config.orchestrator_timeout_seconds);
    Ok(Arc::new(move |run| {
        let (command, model, files) = (command.clone(), model.clone(), Arc::clone(&files));
        let rewritten = describe_catalogue(&catalogue.current(), &described, &files.system_prompt);
        Box::pin(async move {
            rewritten.map_err(|error| {
                tr!(
                    "prompt de l'orchestrateur : {error}",
                    "prompt of the orchestrator: {error}"
                )
            })?;
            // Giving the run up drops its process, which is killed with it.
            match tokio::time::timeout(timeout, run_claude(&command, &model, &files, run)).await {
                Ok(result) => result,
                Err(_) => Err(tr!(
                    "claude -p n'a pas répondu en {} s (orchestratorTimeoutSeconds)",
                    "claude -p did not answer within {} s (orchestratorTimeoutSeconds)",
                    timeout.as_secs()
                )),
            }
        })
    }))
}

/// Rewrites the orchestrator's prompt file when the catalogue or the language in service is no
/// longer the one it was written for.
fn describe_catalogue(
    current: &Arc<Catalogue>,
    described: &Mutex<(Arc<Catalogue>, Language)>,
    system_prompt: &str,
) -> std::io::Result<()> {
    let mut described = described.lock().unwrap_or_else(PoisonError::into_inner);
    let language = language::current();
    if !Arc::ptr_eq(current, &described.0) || described.1 != language {
        // A run just started may be reading it: the file changes in one step.
        write_whole(Path::new(system_prompt), build_orchestrator_prompt(current.values()))?;
        *described = (Arc::clone(current), language);
    }
    Ok(())
}

fn current_step_name(agent: &AgentRecord) -> &str {
    agent.current_step().map_or("none", |step| step.name.as_str())
}

struct Inner {
    store: Store,
    service: AgentService,
    run: ClaudeRunner,
    model: String,
    max_context_tokens: u64,
    now: Clock,
    /// Chat and anomaly runs wait for one another here, in the order they came.
    queue: tokio::sync::Mutex<()>,
    pending: AtomicUsize,
}

/// Lowers the busy flag when the run it stands for ends, whichever way.
struct BusyGuard(Orchestrator);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        if self.0.0.pending.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.0.0.service.emit(Event::Busy(false));
        }
    }
}

#[derive(Clone)]
pub struct Orchestrator(Arc<Inner>);

impl Orchestrator {
    pub fn new(store: Store, service: AgentService, run: ClaudeRunner, config: &AtlasConfig, now: Clock) -> Self {
        Self(Arc::new(Inner {
            store,
            service,
            run,
            model: config.orchestrator_model.clone(),
            max_context_tokens: config.orchestrator_max_context_tokens,
            now,
            queue: tokio::sync::Mutex::new(()),
            pending: AtomicUsize::new(0),
        }))
    }

    /// Has the service wake this orchestrator: anomalies wait their turn, a validation runs at once.
    pub fn listen(&self) {
        let (wakeups, mut received) = mpsc::unbounded_channel();
        self.0.service.wake(wakeups);
        let orchestrator = self.clone();
        tokio::spawn(async move {
            while let Some(wakeup) = received.recv().await {
                match wakeup {
                    Wakeup::Anomaly(anomaly) => drop(tokio::spawn(orchestrator.handle_anomaly(anomaly))),
                    Wakeup::Approval(request) => drop(tokio::spawn(orchestrator.handle_approval(request))),
                }
            }
        });
    }

    fn raise_busy(&self) -> BusyGuard {
        if self.0.pending.fetch_add(1, Ordering::SeqCst) == 0 {
            self.0.service.emit(Event::Busy(true));
        }
        BusyGuard(self.clone())
    }

    /// Tells the master about a failed run instead of losing it.
    fn report(&self, outcome: Result<()>) {
        let Err(error) = outcome else {
            return;
        };
        if let Err(error) = self.0.service.warn_master(
            &tr!(
                "L'orchestrateur a rencontré une erreur : {error}",
                "The orchestrator ran into an error: {error}"
            ),
            None,
        ) {
            tracing::error!(
                "{}",
                tr!(
                    "erreur de l'orchestrateur non signalée : {error}",
                    "error of the orchestrator not reported: {error}"
                )
            );
        }
    }

    /// A run that waits for the ones before it. The busy flag is raised now, not when the run
    /// starts: a queued message already shows the orchestrator busy.
    fn enqueue<F>(&self, task: F) -> impl Future<Output = ()> + Send + 'static
    where
        F: Future<Output = Result<()>> + Send + 'static,
    {
        let busy = self.raise_busy();
        let orchestrator = self.clone();
        async move {
            let _busy = busy;
            let _turn = orchestrator.0.queue.lock().await;
            orchestrator.report(task.await);
        }
    }

    pub fn is_busy(&self) -> bool {
        self.0.pending.load(Ordering::SeqCst) > 0
    }

    async fn run(&self, prompt: String, resume_id: Option<String>) -> Result<ClaudeResult> {
        (self.0.run)(ClaudeRun { prompt, resume_id })
            .await
            .map_err(Error::Internal)
    }

    /// Keeps what a run cost, and returns the context size it ended with (tokens of one turn).
    fn record(&self, kind: OrchestratorRunKind, result: &ClaudeResult) -> Result<u64> {
        let Some(usage) = result.usage else {
            return Ok(0);
        };
        let turns = result.turns.unwrap_or(1).max(1);
        self.0.store.add_orchestrator_run(&OrchestratorRun {
            kind,
            model: self.0.model.clone(),
            usage,
            turns,
            duration_ms: result.duration_ms.unwrap_or(0),
        })?;
        let context = usage.input_tokens + usage.cache_read_tokens + usage.cache_write_tokens;
        Ok((context as f64 / f64::from(turns)).round() as u64)
    }

    /// Messages of the current conversation; the older ones stay in the database.
    pub fn conversation(&self) -> Result<Vec<ChatMessage>> {
        self.0
            .store
            .list_messages(self.0.store.get_value(CLEARED_AT_KEY)?.as_deref())
    }

    /// Starts a new conversation: the next message opens a fresh Claude Code session instead of
    /// resuming the old one. Past messages stay in the database but leave the thread.
    pub fn reset(&self) -> Result<()> {
        if self.is_busy() {
            refuse!(
                "l'orchestrateur réfléchit encore : réessaie après sa réponse",
                "the orchestrator is still thinking: try again after its answer"
            );
        }
        self.0.store.delete_value(ORCHESTRATOR_SESSION_KEY)?;
        self.0.store.delete_value(CONTEXT_TOKENS_KEY)?;
        self.0.store.set_value(CLEARED_AT_KEY, &iso((self.0.now)()))?;
        self.0.service.emit(Event::Reset);
        Ok(())
    }

    /// The chat session to resume, none when the conversation is new or has grown past the limit.
    /// A session that only grows makes every later message pay for all the earlier ones, while the
    /// state it would remember lives in Atlas anyway.
    fn chat_session(&self) -> Result<Option<String>> {
        let session_id = self.0.store.get_value(ORCHESTRATOR_SESSION_KEY)?;
        let context_tokens: u64 = self
            .0
            .store
            .get_value(CONTEXT_TOKENS_KEY)?
            .and_then(|tokens| tokens.parse().ok())
            .unwrap_or(0);
        Ok(session_id.filter(|_| context_tokens <= self.0.max_context_tokens))
    }

    /// Makes the next chat message open a fresh session, the thread kept. Called when the daemon
    /// starts: a resumed session still holds the tools as they were when it loaded them, and would
    /// go on denying what Atlas has learnt to do since.
    pub fn rotate_chat_session(&self) -> Result<()> {
        if self.0.store.get_value(ORCHESTRATOR_SESSION_KEY)?.is_some() {
            self.0.store.set_value(CONTEXT_TOKENS_KEY, &u64::MAX.to_string())?;
        }
        Ok(())
    }

    /// The last exchanges, for a session that replaces an older one.
    fn recap(&self) -> Result<String> {
        let conversation = self.conversation()?;
        let earlier = &conversation[..conversation.len().saturating_sub(1)];
        let lines: Vec<String> = earlier[earlier.len().saturating_sub(ROTATION_RECAP_MESSAGES)..]
            .iter()
            .map(|message| {
                format!(
                    "{}: {}",
                    if message.role == MessageRole::User {
                        "Master"
                    } else {
                        "You"
                    },
                    message.text
                )
            })
            .collect();
        if lines.is_empty() {
            return Ok(String::new());
        }
        Ok(format!(
            "Last exchanges of this conversation, taken over in a new session. Your tools may have changed since: what you called impossible there may no longer be.\n{}\n\nMessage of the master:\n",
            lines.join("\n")
        ))
    }

    /// A chat message from the master: one resumed conversation, so the orchestrator keeps context.
    pub fn chat(&self, text: String, mentions: Vec<String>) -> impl Future<Output = ()> + Send + 'static {
        let orchestrator = self.clone();
        self.enqueue(async move {
            let store = &orchestrator.0.store;
            let resume_id = orchestrator.chat_session()?;
            let is_rotated = resume_id.is_none() && store.get_value(ORCHESTRATOR_SESSION_KEY)?.is_some();
            let recap = if is_rotated {
                orchestrator.recap()?
            } else {
                String::new()
            };
            let prompt = format!("{recap}{}", orchestrator.with_mentions(&text, &mentions)?);
            let result = orchestrator.run(prompt, resume_id).await?;
            if let Some(session_id) = &result.session_id {
                store.set_value(ORCHESTRATOR_SESSION_KEY, session_id)?;
            }
            store.set_value(
                CONTEXT_TOKENS_KEY,
                &orchestrator.record(OrchestratorRunKind::Chat, &result)?.to_string(),
            )?;
            let reply = match result.text.trim() {
                "" if result.is_error => "Je n'ai pas pu traiter ce message.",
                "" => "C'est fait.",
                reply => reply,
            };
            let message = store.add_message(MessageRole::Assistant, reply, None, MessageExtras::default())?;
            orchestrator.0.service.emit(Event::Message(Box::new(message)));
            Ok(())
        })
    }

    /// Appends what the orchestrator needs to act on the agents the master mentioned with @.
    pub fn with_mentions(&self, text: &str, mentions: &[String]) -> Result<String> {
        let mut seen: Vec<&String> = Vec::new();
        let mut lines = Vec::new();
        for agent_id in mentions {
            if seen.contains(&agent_id) {
                continue;
            }
            seen.push(agent_id);
            if let Some(agent) = self.0.store.get_agent(agent_id)? {
                lines.push(format!(
                    "- {} ({}): agentId {}, step {}, status {}",
                    agent.ticket_ref,
                    agent.title,
                    agent.id,
                    current_step_name(&agent),
                    agent.status.as_str()
                ));
            }
        }
        Ok(if lines.is_empty() {
            text.to_owned()
        } else {
            format!("{text}\n\nAgents mentioned:\n{}", lines.join("\n"))
        })
    }

    /// An anomaly: a one-shot run that reads the state, tries to unblock and warns the master if it cannot.
    pub fn handle_anomaly(&self, anomaly: Anomaly) -> impl Future<Output = ()> + Send + 'static {
        let orchestrator = self.clone();
        self.enqueue(async move {
            let result = orchestrator.run(orchestrator.describe_anomaly(&anomaly)?, None).await?;
            orchestrator.record(OrchestratorRunKind::Anomaly, &result)?;
            if result.is_error {
                let reason = if result.text.is_empty() {
                    tr!("échec de l'analyse de l'anomalie", "the analysis of the anomaly failed")
                } else {
                    result.text
                };
                return Err(Error::Internal(reason));
            }
            Ok(())
        })
    }

    pub fn describe_anomaly(&self, anomaly: &Anomaly) -> Result<String> {
        let (agent_id, exit_code) = match anomaly {
            Anomaly::Idle { agent_id } => return self.describe_idle(agent_id),
            Anomaly::Threshold { agent_id } => (agent_id, None),
            Anomaly::SessionDied { agent_id, exit_code } => (agent_id, Some(exit_code)),
        };
        let agent = self.0.store.require_agent(agent_id)?;
        let step_name = current_step_name(&agent);
        let situation = match exit_code {
            None => {
                let threshold = match agent.current_step() {
                    Some(step) => require_brick(&self.0.service.catalogue(), &step.brick_id)?.threshold_minutes,
                    None => 0,
                };
                let minutes = (active_step_ms(&agent, (self.0.now)()) as f64 / MS_PER_MINUTE).round();
                format!("has been in the step {step_name} for {minutes} min, past the threshold of {threshold} min.")
            }
            Some(exit_code) => {
                let code = exit_code.map_or_else(|| "unknown".to_owned(), |code| code.to_string());
                format!("lost its session (exit code {code}) during the step {step_name}.")
            }
        };
        let is_open = if self.0.service.has_live_session(&agent.id) {
            "yes"
        } else {
            "no"
        };
        Ok([
            format!("Anomaly: the agent {} working on {} ({}) {situation}", agent.id, agent.ticket_ref, agent.title),
            format!("Session open: {is_open}."),
            "Act once:".to_owned(),
            "- Session closed: reopen it with resume_agent, which keeps the context of the agent.".to_owned(),
            "- Session closed and the previous restart already failed (get_agent shows several dead sessions in a row on this step): do not insist, warn the master.".to_owned(),
            "- Session open and threshold passed: the agent may still be working. Do not restart it, you would make it lose its context: warn the master, who will look at its session.".to_owned(),
            "- restart_agent only if resume_agent is refused or if the session cannot be resumed.".to_owned(),
            "To warn the master, use notify_master with the agentId: say what is happening and ask him which action to take.".to_owned(),
            "Do nothing else.".to_owned(),
        ]
        .join("\n"))
    }

    fn describe_idle(&self, agent_id: &str) -> Result<String> {
        let agent = self.0.store.require_agent(agent_id)?;
        Ok([
            format!(
                "Anomaly: the session of the agent {} working on {} ({}) waits for input during the step {}, while it asked the master no question. Nothing will move it on by itself.",
                agent.id,
                agent.ticket_ref,
                agent.title,
                current_step_name(&agent)
            ),
            "Read its screen first with read_agent_terminal, then act once:".to_owned(),
            "- A launch error, or a session that received no instruction: restart_agent.".to_owned(),
            "- The agent ended its turn without closing the step: send_to_agent, so that it calls next_step and finishes the step.".to_owned(),
            "- The agent asks a question in the terminal, or the master is talking to it: notify_master with the agentId, without summing up the question.".to_owned(),
            "- get_agent shows that this step was already restarted for the same reason: do not insist, notify_master with what the screen shows.".to_owned(),
            "Do nothing else.".to_owned(),
        ]
        .join("\n"))
    }

    /// A step waiting for the orchestrator's validation: a one-shot run that holds everything needed
    /// to decide. If the run ends without a decision, the step is approved and the workflow kept.
    pub fn handle_approval(&self, request: ApprovalRequest) -> impl Future<Output = ()> + Send + 'static {
        let busy = self.raise_busy();
        let orchestrator = self.clone();
        async move {
            let _busy = busy;
            let decide = async {
                let result = orchestrator
                    .run(orchestrator.describe_approval(&request)?, None)
                    .await?;
                orchestrator.record(OrchestratorRunKind::Approval, &result)?;
                if result.is_error {
                    let reason = if result.text.is_empty() {
                        tr!("échec de la validation", "the validation failed")
                    } else {
                        result.text
                    };
                    return Err(Error::Internal(reason));
                }
                Ok(())
            };
            let outcome = decide.await;
            let settled = orchestrator.0.service.settle_approval(&request.agent_id, request.id);
            orchestrator.report(outcome.and(settled));
        }
    }

    pub fn describe_approval(&self, request: &ApprovalRequest) -> Result<String> {
        let agent = self.0.store.require_agent(&request.agent_id)?;
        let Some(step) = agent.steps.iter().find(|candidate| candidate.id == request.step_id) else {
            return Err(Error::internal(tr!(
                "étape inconnue : {}",
                "unknown step: {}",
                request.step_id
            )));
        };
        let upcoming: Vec<&str> = agent
            .upcoming_steps()
            .map(|candidate| candidate.brick_id.as_str())
            .collect();
        let mut lines = vec![
            format!(
                "Validation requested: the agent {} on {} ({}) hands in its step {}.",
                agent.id, agent.ticket_ref, agent.title, step.name
            ),
            match agent.note.as_deref().filter(|note| !note.is_empty()) {
                Some(note) => format!("Note of the master at launch: {note}"),
                None => "No note from the master.".to_owned(),
            },
            "Fields handed in:".to_owned(),
        ];
        lines.extend(
            step.output
                .iter()
                .flatten()
                .map(|(field, value)| format!("- {field}: {value}")),
        );
        lines.push(format!(
            "Steps that remain: {}.",
            if upcoming.is_empty() {
                "none".to_owned()
            } else {
                upcoming.join(", ")
            }
        ));
        lines.push(
            "Decide now with approve_step, once, according to your validation rules. Do nothing else.".to_owned(),
        );
        Ok(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests;
