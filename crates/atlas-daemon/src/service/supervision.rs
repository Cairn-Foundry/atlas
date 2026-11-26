// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Watching the board: steps over their threshold, stopped and archived agents, broadcasts.

use crate::language::tr;
use std::time::Duration;

use crate::clock::iso;
use crate::contract::{AgentStatus, AgentView};
use crate::error::Result;
use crate::store::{AgentPatch, AgentRecord, to_agent_view};
use crate::workflow::is_over_threshold;

use super::{AgentService, Anomaly, ApprovalDecision, Event, Wakeup};

impl AgentService {
    /// Stops the agent where it stands: its session closes, it stays on the board until resumed.
    pub fn stop(&self, agent_id: &str, reason: &str) -> Result<AgentRecord> {
        let agent = self.store().require_agent(agent_id)?;
        self.require_not_delivered(&agent)?;
        self.close_session(agent_id, "stopped by the orchestrator", Duration::ZERO)?;
        if let Some(pending) = self.approvals().remove(agent_id) {
            let _ = pending
                .settle
                .send(ApprovalDecision::Rejected(tr!("agent arrêté", "agent stopped")));
        }
        self.update(
            agent_id,
            AgentPatch {
                status: Some(AgentStatus::Blocked),
                status_reason: Some(Some(tr!("arrêté : {reason}", "stopped: {reason}"))),
                waiting_since: Some(Some(agent.waiting_since.clone().unwrap_or_else(|| iso(self.now())))),
                ..AgentPatch::default()
            },
        )
    }

    /// Takes the agent off the board. Its session closes; its branch and folder stay.
    pub fn archive(&self, agent_id: &str) -> Result<()> {
        self.close_session(agent_id, "archived", Duration::ZERO)?;
        if let Some(pending) = self.approvals().remove(agent_id) {
            let _ = pending.settle.send(ApprovalDecision::Approved);
        }
        self.update(
            agent_id,
            AgentPatch {
                archived_at: Some(Some(iso(self.now()))),
                ..AgentPatch::default()
            },
        )?;
        self.emit(Event::Archived(agent_id.to_owned()));
        Ok(())
    }

    /// Agents a broadcast reaches: those on the board that have not delivered their work.
    pub fn active_agent_count(&self) -> Result<usize> {
        Ok(self
            .store()
            .list_agents(false)?
            .iter()
            .filter(|agent| agent.status != AgentStatus::Done)
            .count())
    }

    pub fn broadcast(&self, text: &str) -> Result<usize> {
        self.store().add_broadcast(text)?;
        self.active_agent_count()
    }

    /// One supervisor pass: flags each agent over its step threshold once per step.
    pub fn supervise(&self) -> Result<Vec<Anomaly>> {
        let now = self.now();
        let mut found = Vec::new();
        for agent in self.store().list_agents(false)? {
            if agent.status != AgentStatus::Working || agent.threshold_alerted {
                continue;
            }
            let Some(step) = agent.current_step() else {
                continue;
            };
            // A brick the master deleted costs this agent its alert, not the agents listed after it.
            let brick = match self.brick(&step.brick_id) {
                Ok(brick) => brick,
                Err(error) => {
                    tracing::warn!(
                        "{}",
                        tr!(
                            "supervision de {} : {error}",
                            "supervision of {}: {error}",
                            agent.ticket_ref
                        )
                    );
                    continue;
                }
            };
            if !is_over_threshold(&brick, &agent, now) {
                continue;
            }
            self.update(
                &agent.id,
                AgentPatch {
                    threshold_alerted: Some(true),
                    ..AgentPatch::default()
                },
            )?;
            let anomaly = Anomaly::Threshold {
                agent_id: agent.id.clone(),
            };
            self.send_wakeup(Wakeup::Anomaly(anomaly.clone()));
            found.push(anomaly);
        }
        Ok(found)
    }

    pub fn views(&self) -> Result<Vec<AgentView>> {
        Ok(self.store().list_agents(false)?.iter().map(to_agent_view).collect())
    }
}
