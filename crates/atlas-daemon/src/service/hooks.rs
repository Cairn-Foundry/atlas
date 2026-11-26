// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What Claude Code's hooks report about an agent's session, and what Atlas answers.

use crate::clock::elapsed_ms;
use crate::contract::AgentStatus;
use crate::error::Result;
use crate::prompts::session_start_brief;
use crate::store::{AgentPatch, AgentRecord};

use super::{AgentService, Anomaly, Event, Wakeup};

const MAX_STOP_BLOCKS: u32 = 3;

impl AgentService {
    /// The agent back at work after waiting for the master: the wait is counted apart from the step.
    pub(super) fn back_to_work_patch(&self, agent: &AgentRecord) -> AgentPatch {
        let waited = agent
            .waiting_since
            .as_deref()
            .map_or(0, |since| elapsed_ms(since, self.now()));
        AgentPatch {
            status: Some(AgentStatus::Working),
            status_reason: Some(None),
            waiting_since: Some(None),
            step_wait_ms: Some(agent.step_wait_ms + waited),
            ..AgentPatch::default()
        }
    }

    /// UserPromptSubmit: the master answered in the terminal, the agent is back to work.
    pub fn on_user_prompt(&self, agent_id: &str, prompt: Option<&str>) -> Result<()> {
        if let (Some(live), Some(prompt)) = (self.live(agent_id), prompt) {
            let mut state = live.state();
            if let Some(index) = state
                .atlas_prompts
                .iter()
                .position(|candidate| candidate.trim() == prompt.trim())
            {
                state.atlas_prompts.remove(index);
                return Ok(());
            }
        }
        let agent = self.store().require_agent(agent_id)?;
        if matches!(agent.status, AgentStatus::Waiting | AgentStatus::Blocked) {
            self.update(agent_id, self.back_to_work_patch(&agent))?;
        }
        Ok(())
    }

    /// Stop hook: an agent with work left is sent back to `next_step`. Capped so a confused agent
    /// cannot loop forever. Returns the reason the stop is refused, none when the agent may stop.
    pub fn on_stop(&self, agent_id: &str) -> Result<Option<&'static str>> {
        let agent = self.store().require_agent(agent_id)?;
        if agent.status != AgentStatus::Working || agent.stop_blocks >= MAX_STOP_BLOCKS {
            return Ok(None);
        }
        self.update(
            agent_id,
            AgentPatch {
                stop_blocks: Some(agent.stop_blocks + 1),
                ..AgentPatch::default()
            },
        )?;
        Ok(Some(
            "Ton étape Atlas n'est pas close, et personne ne reprendra ton travail si tu t'arrêtes ici. \
             Si tu n'as pas encore lu ses instructions ou si tu as un doute sur ce qui est attendu, appelle next_step. \
             S'il reste du travail, continue-le. Si l'étape est finie et vérifiée, appelle complete_step. \
             S'il te faut une décision du maître, appelle ask_user ; si tu ne peux pas avancer, block.",
        ))
    }

    /// PostToolUse: delivers the master's broadcasts while the agent works.
    pub fn on_post_tool_use(&self, agent_id: &str) -> Result<Option<String>> {
        if let Some(live) = self.live(agent_id) {
            live.state().is_idle_reported = false;
        }
        let messages = self.store().take_pending_broadcasts(agent_id)?;
        if messages.is_empty() {
            return Ok(None);
        }
        Ok(Some(
            messages
                .iter()
                .map(|text| format!("Message of the master to all the agents: {text}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ))
    }

    /// SessionStart: who the session is, where it stands in its workflow, and the standing rules.
    pub fn on_session_start(&self, agent_id: &str) -> Result<String> {
        let agent = self.store().require_agent(agent_id)?;
        let current = match agent.current_step() {
            Some(step) => Some((step, self.brick(&step.brick_id)?)),
            None => None,
        };
        Ok(session_start_brief(
            &agent,
            current.as_ref().map(|(step, brick)| (*step, brick)),
            &self.projects().list_global_notes()?,
            &self.projects().list()?,
        ))
    }

    /// Notification (idle prompt): the session waits for input. Expected while the master is asked;
    /// while the agent should be working it is stuck (a launch that lost its prompt, a turn ended
    /// without closing the step), and nobody would notice: the orchestrator is woken.
    pub fn on_idle(&self, agent_id: &str) -> Result<()> {
        let agent = self.store().require_agent(agent_id)?;
        self.emit(Event::Agent(Box::new(agent.clone())));
        let Some(live) = self.live(agent_id) else {
            return Ok(());
        };
        if agent.status != AgentStatus::Working || agent.current_step().is_none() {
            return Ok(());
        }
        if std::mem::replace(&mut live.state().is_idle_reported, true) {
            return Ok(());
        }
        self.send_wakeup(Wakeup::Anomaly(Anomaly::Idle {
            agent_id: agent_id.to_owned(),
        }));
        Ok(())
    }
}
