// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Claude Code sessions: opening, closing and resuming them, and what their terminals show.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use foundry_pty::{PtyEvent, PtySize, TerminalMirror};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::brick::Brick;
use crate::catalogue::{SKILLS_DIR, SUBAGENTS_DIR};
use crate::claude::{claude_env, write_agent_settings, write_atlas_mcp_config, write_session_plugin};
use crate::clock::iso;
use crate::contract::{AgentStatus, MessageAction, TERMINAL_SCROLLBACK_LINES};
use crate::error::{Error, Result, refuse};
use crate::language::tr;
use crate::prompts::{NEW_STEP_PROMPT, RESUME_PROMPT, command_prompt, launch_prompt};
use crate::store::{AgentPatch, AgentRecord, NewSession, StepPatch, StepRecord};
use crate::terminal::{INITIAL_SIZE, MAX_SIZE, SpawnRequest, Utf8Chunks};
use crate::workflow::withheld_tools;

use super::{AgentService, Anomaly, LiveSession, LiveState, TerminalEvent, Wakeup};

/// Why an agent that was at work is stopped once the daemon is back.
fn restart_reason() -> String {
    tr!(
        "Atlas a redémarré : la session de l'agent est fermée, rouvrez-la pour qu'il reprenne",
        "Atlas restarted: the session of the agent is closed, reopen it so that it resumes"
    )
}
/// Margin between a session's close and the opening of the next one.
const REOPEN_MARGIN: Duration = Duration::from_millis(100);
const BRACKETED_PASTE_START: &str = "\u{1b}[200~";
const BRACKETED_PASTE_END: &str = "\u{1b}[201~";
/// Delay between a pasted message and the Enter that sends it, so Claude Code has read the paste.
pub(super) const PASTE_SUBMIT_DELAY: Duration = Duration::from_millis(300);

/// An argument as a shell would need it, to show a command line that can be read and rerun.
pub(super) fn quote_argument(argument: &str) -> String {
    let is_plain = !argument.is_empty()
        && argument
            .chars()
            .all(|character| character.is_alphanumeric() || "_@%+=:,./-".contains(character));
    if is_plain {
        argument.to_owned()
    } else {
        format!("'{}'", argument.replace('\'', "'\\''"))
    }
}

impl AgentService {
    /// Opens the session of the agent's current step: the session the step already ran in, else the
    /// brick's slash command in a fresh one, else the latest session of the brick's group and model
    /// (which knows the code already) unless the brick wants its own, else a fresh one. `is_fresh`
    /// skips every reuse.
    pub(super) fn open_step_session(
        &self,
        agent_id: &str,
        is_fresh: bool,
        prompt: Option<String>,
    ) -> Result<AgentRecord> {
        let agent = self.store().require_agent(agent_id)?;
        let step = self.require_current(&agent)?;
        let brick = &self.brick(&step.brick_id)?;
        let own = match &step.session_id {
            Some(session_id) if !is_fresh => self.store().get_session(session_id)?,
            _ => None,
        };
        if let Some(own) = own {
            return self.start_session(
                &agent,
                step,
                brick,
                prompt.unwrap_or_else(|| RESUME_PROMPT.to_owned()),
                Some(own.id),
            );
        }
        if let Some(command) = &brick.command {
            return self.start_session(&agent, step, brick, command_prompt(&agent, step, brick, command), None);
        }
        let shared = if is_fresh || brick.fresh_session {
            None
        } else {
            self.store()
                .find_group_session(&agent.id, &brick.session, &step.model)?
        };
        if let Some(shared) = shared {
            return self.start_session(
                &agent,
                step,
                brick,
                prompt.unwrap_or_else(|| NEW_STEP_PROMPT.to_owned()),
                Some(shared.id),
            );
        }
        self.start_session(&agent, step, brick, launch_prompt(&agent), None)
    }

    pub(super) fn agent_dir(&self, agent_id: &str) -> Result<PathBuf> {
        let dir = self.0.deps.home.join("agents").join(agent_id);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// The MCP config and hook settings of the agent's sessions, written under `~/.atlas/agents/<id>`.
    fn write_session_files(&self, agent: &AgentRecord) -> Result<(String, String)> {
        let dir = self.agent_dir(&agent.id)?;
        let mcp_config = dir.join("mcp.json");
        let settings = dir.join("settings.json");
        let deps = &self.0.deps;
        write_atlas_mcp_config(
            &mcp_config,
            &format!("{}/mcp/agent/{}", deps.daemon_url, agent.id),
            &deps.token,
        )?;
        write_agent_settings(&settings, &deps.hook_executable)?;
        Ok((
            mcp_config.to_string_lossy().into_owned(),
            settings.to_string_lossy().into_owned(),
        ))
    }

    fn start_session(
        &self,
        agent: &AgentRecord,
        step: &StepRecord,
        brick: &Brick,
        prompt: String,
        resume_id: Option<String>,
    ) -> Result<AgentRecord> {
        if self.has_live_session(&agent.id) {
            refuse!(
                "l'agent {} a déjà une session ouverte",
                "the agent {} already has an open session",
                agent.ticket_ref
            );
        }
        let deps = &self.0.deps;
        let session_id = resume_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
        let (mcp_config, settings) = self.write_session_files(agent)?;
        let mut args: Vec<String> = match &resume_id {
            Some(resume_id) => vec!["--resume".to_owned(), resume_id.clone()],
            None => vec!["--session-id".to_owned(), session_id.clone()],
        };
        args.extend(
            [
                "--model",
                &step.model,
                "--mcp-config",
                &mcp_config,
                "--settings",
                &settings,
                "--allowedTools",
                "mcp__atlas",
                "--disallowedTools",
                &withheld_tools(brick).join(","),
                // Agents run unattended in their own folder: no permission prompt stops them.
                "--permission-mode",
                "bypassPermissions",
            ]
            .map(str::to_owned),
        );
        if !brick.skills.is_empty() || !brick.subagent_types.is_empty() {
            let plugin = self.agent_dir(&agent.id)?.join("plugin");
            write_session_plugin(
                &plugin,
                &deps.home.join(SKILLS_DIR),
                &brick.skills,
                &deps.home.join(SUBAGENTS_DIR),
                &brick.subagent_types,
            )?;
            args.extend(["--plugin-dir".to_owned(), plugin.to_string_lossy().into_owned()]);
        }
        for workspace in &agent.workspaces {
            args.extend(["--add-dir".to_owned(), workspace.worktree_path.clone()]);
        }
        args.extend(deps.config.agent_args.iter().cloned());
        // --add-dir is variadic: without the separator it swallows the prompt.
        args.extend(["--".to_owned(), prompt.clone()]);
        let env = claude_env(
            &deps.environment,
            &[
                ("ATLAS_URL", &deps.daemon_url),
                ("ATLAS_TOKEN", &deps.token),
                ("ATLAS_AGENT_ID", &agent.id),
                ("ATLAS_SESSION_ID", &session_id),
            ],
        );

        let token = self.0.next_token.fetch_add(1, Ordering::Relaxed);
        let reports = self.0.pty_reports.clone();
        let reporting_agent = agent.id.clone();
        let command_line = std::iter::once(&deps.config.claude_command)
            .chain(&args)
            .map(|argument| quote_argument(argument))
            .collect::<Vec<_>>()
            .join(" ");
        let request = SpawnRequest {
            command: deps.config.claude_command.clone(),
            args,
            cwd: agent.worktree_path.clone(),
            env,
            size: INITIAL_SIZE,
        };
        let process = deps
            .launcher
            .spawn(
                &request,
                Box::new(move |event| {
                    let _ = reports.send((reporting_agent.clone(), token, event));
                }),
            )
            .map_err(Error::Refusal)?;
        let live = Arc::new(LiveSession {
            token,
            session_id: session_id.clone(),
            group: brick.session.clone(),
            model: step.model.clone(),
            has_subagents: brick.subagents,
            skills: brick.skills.clone(),
            subagent_types: brick.subagent_types.clone(),
            command_line,
            process,
            state: Mutex::new(LiveState {
                atlas_prompts: vec![prompt],
                mirror: TerminalMirror::new(INITIAL_SIZE, TERMINAL_SCROLLBACK_LINES),
                decoder: Utf8Chunks::default(),
                is_idle_reported: false,
            }),
        });
        self.live_sessions().insert(agent.id.clone(), live);

        let is_known = match &resume_id {
            Some(resume_id) => self.store().get_session(resume_id)?.is_some(),
            None => false,
        };
        if is_known {
            self.store().reopen_session(&session_id)?;
        } else {
            self.store().create_session(&NewSession {
                id: session_id.clone(),
                agent_id: agent.id.clone(),
                group: brick.session.clone(),
                model: step.model.clone(),
                step_id: step.brick_id.clone(),
            })?;
        }
        self.store().update_step(
            &step.id,
            StepPatch {
                session_id: Some(Some(session_id.clone())),
                ..StepPatch::default()
            },
        )?;
        self.update(
            &agent.id,
            AgentPatch {
                live_session_id: Some(Some(session_id)),
                stop_blocks: Some(0),
                ..AgentPatch::default()
            },
        )
    }

    /// What a session's terminal reported, handed over by its reading threads.
    pub(super) fn on_pty_event(&self, agent_id: &str, token: u64, event: PtyEvent) -> Result<()> {
        let live = self.live(agent_id).filter(|live| live.token == token);
        match (event, live) {
            (PtyEvent::Data(data), Some(live)) => {
                let text = {
                    let mut state = live.state();
                    state.mirror.write(&data);
                    state.decoder.push(&data)
                };
                if !text.is_empty() {
                    let _ = self.0.terminal.send(TerminalEvent::Output {
                        agent_id: agent_id.to_owned(),
                        data: text,
                    });
                }
                Ok(())
            }
            // What a session prints once Atlas closed it is of no use to anyone.
            (PtyEvent::Data(_), None) => Ok(()),
            (PtyEvent::Exit(code), Some(live)) => self.handle_exit(agent_id, &live, code),
            (PtyEvent::Exit(code), None) => {
                // A session Atlas closed: its end is told unless another one took its place meanwhile.
                if !self.has_live_session(agent_id) {
                    let _ = self.0.terminal.send(TerminalEvent::Exit {
                        agent_id: agent_id.to_owned(),
                        code,
                    });
                }
                Ok(())
            }
        }
    }

    /// A session that ended by itself: the agent loses it, and the orchestrator is woken if it was at work.
    fn handle_exit(&self, agent_id: &str, live: &LiveSession, code: Option<i32>) -> Result<()> {
        self.live_sessions().remove(agent_id);
        let _ = self.0.terminal.send(TerminalEvent::Exit {
            agent_id: agent_id.to_owned(),
            code,
        });
        let Some(agent) = self.store().get_agent(agent_id)? else {
            return Ok(());
        };
        let outcome = match code {
            Some(0) => "exited".to_owned(),
            Some(code) => format!("exited with code {code}"),
            None => "exited with code unknown".to_owned(),
        };
        self.store().end_session(&live.session_id, &outcome)?;
        if agent.live_session_id.as_deref() == Some(live.session_id.as_str()) {
            self.update(
                agent_id,
                AgentPatch {
                    live_session_id: Some(None),
                    ..AgentPatch::default()
                },
            )?;
        }
        if agent.status == AgentStatus::Working {
            self.send_wakeup(Wakeup::Anomaly(Anomaly::SessionDied {
                agent_id: agent_id.to_owned(),
                exit_code: code,
            }));
        }
        Ok(())
    }

    /// Closes the agent's session, at once or after `delay`; nothing happens when it has none.
    pub(super) fn close_session(&self, agent_id: &str, outcome: &str, delay: Duration) -> Result<()> {
        let Some(live) = self.live_sessions().remove(agent_id) else {
            return Ok(());
        };
        self.store().end_session(&live.session_id, outcome)?;
        self.update(
            agent_id,
            AgentPatch {
                live_session_id: Some(None),
                ..AgentPatch::default()
            },
        )?;
        if delay.is_zero() {
            live.process.kill();
        } else {
            self.after(delay, move |_| live.process.kill());
        }
        Ok(())
    }

    /// Closes the live session once its last tool reply is delivered, then opens the current step's session.
    pub(super) fn move_to_new_session(&self, agent_id: &str, outcome: &str) -> Result<()> {
        let close_delay = self.0.deps.close_delay;
        self.close_session(agent_id, outcome, close_delay)?;
        let agent_id = agent_id.to_owned();
        self.after(close_delay + REOPEN_MARGIN, move |service| {
            if let Err(error) = service.open_step_session_safely(&agent_id) {
                tracing::error!(
                    "{}",
                    tr!(
                        "ouverture de la session de {agent_id} : {error}",
                        "opening the session of {agent_id}: {error}"
                    )
                );
            }
        });
        Ok(())
    }

    /// What redraws the session's terminal as it stands, with everything it prints from then on;
    /// none when the agent has no open session.
    pub fn attach_terminal(&self, agent_id: &str) -> Option<(String, broadcast::Receiver<TerminalEvent>)> {
        let live = self.live(agent_id)?;
        // Nothing is printed between the snapshot and the subscription: both happen before the runtime moves on.
        let snapshot = live.state().mirror.snapshot();
        Some((snapshot, self.subscribe_terminal()))
    }

    pub fn write_to_terminal(&self, agent_id: &str, data: &str) {
        if let Some(live) = self.live(agent_id) {
            live.process.write(data.as_bytes());
        }
    }

    pub fn resize_terminal(&self, agent_id: &str, cols: u16, rows: u16) {
        if let Some(live) = self.live(agent_id) {
            let size = PtySize {
                cols: cols.min(MAX_SIZE.cols),
                rows: rows.min(MAX_SIZE.rows),
            };
            live.process.resize(size);
            live.state().mirror.resize(size);
        }
    }

    pub fn has_live_session(&self, agent_id: &str) -> bool {
        self.live_sessions().contains_key(agent_id)
    }

    /// What the orchestrator sees of an open session: the command it was started with and the end of its screen.
    pub fn read_terminal(&self, agent_id: &str, line_count: usize) -> Result<(String, String)> {
        let agent = self.store().require_agent(agent_id)?;
        let Some(live) = self.live(agent_id) else {
            refuse!(
                "{} n'a pas de session ouverte : resume_agent la rouvre",
                "{} has no open session: resume_agent reopens it",
                agent.ticket_ref
            );
        };
        let screen = live.state().mirror.text(line_count);
        Ok((live.command_line.clone(), screen))
    }

    /// Types a message into the agent's session, as the master would: it is queued if the agent is
    /// busy. Without an open session, the session reopens with the message as its prompt.
    pub fn send_to_agent(&self, agent_id: &str, text: &str) -> Result<AgentRecord> {
        let agent = self.store().require_agent(agent_id)?;
        let Some(live) = self.live(agent_id) else {
            return self.resume(agent_id, Some(text.to_owned()));
        };
        live.state().atlas_prompts.push(text.to_owned());
        // Pasted, so a line break does not send half the message; Enter goes once the paste is read.
        live.process
            .write(format!("{BRACKETED_PASTE_START}{text}{BRACKETED_PASTE_END}").as_bytes());
        self.after(PASTE_SUBMIT_DELAY, move |_| live.process.write(b"\r"));
        Ok(agent)
    }

    /// Opens the current step's session where a refusal has nobody to go back to (a timer) or would
    /// leave the agent half made (its launch): the agent is blocked and the master told instead.
    pub(super) fn open_step_session_safely(&self, agent_id: &str) -> Result<()> {
        // A workflow change or a resume may have opened the session while the timer was pending.
        if self.has_live_session(agent_id) {
            return Ok(());
        }
        // Stopped or archived meanwhile: nobody wants a session that would start unattended.
        let agent = self.store().require_agent(agent_id)?;
        if agent.status != AgentStatus::Working || agent.archived_at.is_some() {
            return Ok(());
        }
        let Err(error) = self.open_step_session_or_block(agent_id, false, None) else {
            return Ok(());
        };
        self.warn_master_with(
            &tr!(
                "La session de {} n'a pas pu s'ouvrir : {error}",
                "The session of {} could not open: {error}",
                agent.ticket_ref
            ),
            Some(agent_id),
            vec![MessageAction::Resume],
        )?;
        Ok(())
    }

    /// Opens the current step's session for a caller that gets the refusal back. An agent shown at
    /// work is blocked on it first: nothing moves an agent that works without a session.
    pub(super) fn open_step_session_or_block(
        &self,
        agent_id: &str,
        is_fresh: bool,
        prompt: Option<String>,
    ) -> Result<AgentRecord> {
        let error = match self.open_step_session(agent_id, is_fresh, prompt) {
            Ok(agent) => return Ok(agent),
            Err(error) => error,
        };
        let agent = self.store().require_agent(agent_id)?;
        if agent.status == AgentStatus::Working {
            self.update(
                agent_id,
                AgentPatch {
                    status: Some(AgentStatus::Blocked),
                    status_reason: Some(Some(error.to_string())),
                    waiting_since: Some(Some(agent.waiting_since.unwrap_or_else(|| iso(self.now())))),
                    ..AgentPatch::default()
                },
            )?;
        }
        Err(error)
    }

    /// Reopens the agent's session where it stopped.
    pub fn resume(&self, agent_id: &str, prompt: Option<String>) -> Result<AgentRecord> {
        let agent = self.store().require_agent(agent_id)?;
        if self.has_live_session(agent_id) {
            refuse!(
                "l'agent {} a déjà une session ouverte",
                "the agent {} already has an open session",
                agent.ticket_ref
            );
        }
        if agent.current_step().is_none() {
            let last = agent
                .steps
                .iter()
                .rev()
                .find(|candidate| candidate.session_id.is_some());
            let session = match last.and_then(|last| last.session_id.as_deref()) {
                Some(session_id) => self.store().get_session(session_id)?,
                None => None,
            };
            let (Some(last), Some(session)) = (last, session) else {
                refuse!(
                    "{} n'a aucune session à rouvrir",
                    "{} has no session to reopen",
                    agent.ticket_ref
                );
            };
            let prompt = prompt.unwrap_or_else(|| RESUME_PROMPT.to_owned());
            return self.start_session(&agent, last, &self.brick(&last.brick_id)?, prompt, Some(session.id));
        }
        // A blocked agent whose session is reopened goes back to work: held before a step, or stopped.
        if agent.status == AgentStatus::Blocked {
            self.update(agent_id, self.back_to_work_patch(&agent))?;
        }
        self.open_step_session_or_block(agent_id, false, prompt)
    }

    /// Starts a fresh session on the current step; next_step hands it the agent's file again.
    pub fn restart(&self, agent_id: &str) -> Result<AgentRecord> {
        let agent = self.store().require_agent(agent_id)?;
        if agent.current_step().is_none() {
            refuse!(
                "{} est livré : rouvre sa session avec resume_agent",
                "{} is delivered: reopen its session with resume_agent",
                agent.ticket_ref
            );
        }
        self.close_session(agent_id, "restarted by the orchestrator", Duration::ZERO)?;
        self.update(agent_id, self.back_to_work_patch(&agent))?;
        self.open_step_session_or_block(agent_id, true, None)
    }

    pub fn close(&self, agent_id: &str) -> Result<()> {
        self.close_session(agent_id, "closed by the master", Duration::ZERO)
    }

    /// After a daemon restart no PTY survives: the sessions the store still believes live are
    /// closed, and an agent that was at work is shown stopped, since nothing will move it on until
    /// its session is reopened.
    pub fn recover_after_restart(&self) -> Result<()> {
        for agent in self.store().list_agents(true)? {
            if let Some(session_id) = &agent.live_session_id {
                self.store().end_session(session_id, "daemon restarted")?;
            }
            let was_working =
                agent.status == AgentStatus::Working && agent.archived_at.is_none() && agent.current_step().is_some();
            if agent.live_session_id.is_none() && !was_working {
                continue;
            }
            let mut patch = AgentPatch {
                live_session_id: Some(None),
                ..AgentPatch::default()
            };
            if was_working {
                patch.status = Some(AgentStatus::Blocked);
                patch.status_reason = Some(Some(restart_reason()));
                patch.waiting_since = Some(Some(iso(self.now())));
            }
            self.update(&agent.id, patch)?;
        }
        Ok(())
    }

    pub fn shutdown(&self) {
        let agent_ids: Vec<String> = self.live_sessions().keys().cloned().collect();
        for agent_id in agent_ids {
            if let Err(error) = self.close_session(&agent_id, "daemon stopped", Duration::ZERO) {
                tracing::error!(
                    "{}",
                    tr!(
                        "fermeture de la session de {agent_id} : {error}",
                        "closing the session of {agent_id}: {error}"
                    )
                );
            }
        }
    }
}
