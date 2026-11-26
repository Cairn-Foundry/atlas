// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Steps the orchestrator validates: the agent's call waits for its decision.

use crate::language::tr;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{MutexGuard, PoisonError};
use std::time::Duration;

use tokio::sync::oneshot;

use crate::contract::{MessageKind, StepOutput};
use crate::error::{Result, refuse};
use crate::store::{AgentRecord, StepPatch, StepRecord};

use super::{AgentService, ApprovalDecision, ApprovalRequest, PendingApproval, Wakeup};

/// Forgets a request whose agent hung up before the decision: its tool call was dropped with its
/// session, and a decision taken afterwards would reach nobody.
struct WaitingCall<'a> {
    service: &'a AgentService,
    agent_id: &'a str,
    request_id: u64,
}

impl Drop for WaitingCall<'_> {
    fn drop(&mut self) {
        let mut approvals = self.service.approvals();
        if approvals
            .get(self.agent_id)
            .is_some_and(|pending| pending.id == self.request_id)
        {
            approvals.remove(self.agent_id);
        }
    }
}

impl AgentService {
    pub(super) fn approvals(&self) -> MutexGuard<'_, HashMap<String, PendingApproval>> {
        self.0.approvals.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn has_pending_approval(&self, agent_id: &str) -> bool {
        self.approvals().contains_key(agent_id)
    }

    /// Hands a finished step to the orchestrator and waits for its decision inside the agent's tool
    /// call, so the agent learns what comes next without ending its turn. With nobody listening, or
    /// past the timeout, the step is approved and the workflow stays as it is: the cautious default.
    pub(super) async fn request_approval(
        &self,
        agent: &AgentRecord,
        step: &StepRecord,
        output: &StepOutput,
    ) -> Result<ApprovalDecision> {
        if self.has_pending_approval(&agent.id) {
            refuse!(
                "cette étape attend déjà la décision de l'orchestrateur",
                "this step already waits for the decision of the orchestrator"
            );
        }
        self.store().update_step(
            &step.id,
            StepPatch {
                output: Some(Some(output.clone())),
                ..StepPatch::default()
            },
        )?;
        let (settle, decision) = oneshot::channel();
        let id = self.0.next_token.fetch_add(1, Ordering::Relaxed);
        self.approvals()
            .insert(agent.id.clone(), PendingApproval { id, settle });
        let request = ApprovalRequest {
            id,
            agent_id: agent.id.clone(),
            step_id: step.id.clone(),
        };
        if !self.send_wakeup(Wakeup::Approval(request)) {
            self.approvals().remove(&agent.id);
            return Ok(ApprovalDecision::Approved);
        }
        let agent_id = agent.id.clone();
        self.after(
            Duration::from_secs(self.0.deps.config.approval_timeout_seconds),
            move |service| {
                if let Err(error) = service.settle_approval(&agent_id, id) {
                    tracing::error!(
                        "{}",
                        tr!(
                            "validation sans décision de {agent_id} : {error}",
                            "validation without a decision for {agent_id}: {error}"
                        )
                    );
                }
            },
        );
        let _waiting = WaitingCall {
            service: self,
            agent_id: &agent.id,
            request_id: id,
        };
        // A decision that never comes (the service is going away) leaves the step as it stands.
        Ok(decision.await.unwrap_or(ApprovalDecision::Approved))
    }

    fn take_approval(&self, agent_id: &str) -> Result<oneshot::Sender<ApprovalDecision>> {
        match self.approvals().remove(agent_id) {
            Some(pending) => Ok(pending.settle),
            None => refuse!(
                "aucune étape de cet agent n'attend de validation",
                "no step of this agent waits for a validation"
            ),
        }
    }

    /// The orchestrator validates the waiting step, optionally with new upcoming steps.
    pub fn approve_step(&self, agent_id: &str, reason: &str, steps: Option<&[String]>) -> Result<AgentRecord> {
        if !self.has_pending_approval(agent_id) {
            refuse!(
                "aucune étape de cet agent n'attend de validation",
                "no step of this agent waits for a validation"
            );
        }
        if let Some(steps) = steps {
            self.replace_upcoming(agent_id, steps)?;
        }
        let agent = self.store().require_agent(agent_id)?;
        self.post_card(MessageKind::Decision, &agent, reason)?;
        let _ = self.take_approval(agent_id)?.send(ApprovalDecision::Approved);
        Ok(agent)
    }

    /// The orchestrator sends the waiting step back to its agent.
    pub fn reject_step(&self, agent_id: &str, reason: &str) -> Result<AgentRecord> {
        let settle = self.take_approval(agent_id)?;
        let agent = self.store().require_agent(agent_id)?;
        self.post_card(
            MessageKind::Decision,
            &agent,
            &tr!("Renvoyée à l'agent : {reason}", "Sent back to the agent: {reason}"),
        )?;
        let _ = settle.send(ApprovalDecision::Rejected(reason.to_owned()));
        Ok(agent)
    }

    /// Approves a step the orchestrator left undecided, without touching the workflow. Only the
    /// request `request_id` names: one already decided, or a later one of the same agent, is left alone.
    pub fn settle_approval(&self, agent_id: &str, request_id: u64) -> Result<()> {
        let pending = {
            let mut approvals = self.approvals();
            if approvals.get(agent_id).is_none_or(|pending| pending.id != request_id) {
                return Ok(());
            }
            approvals.remove(agent_id)
        };
        let Some(pending) = pending else {
            return Ok(());
        };
        let agent = self.store().require_agent(agent_id)?;
        self.post_card(
            MessageKind::Decision,
            &agent,
            &tr!(
                "Orchestrateur sans décision : étape validée, parcours inchangé.",
                "No decision from the orchestrator: step validated, workflow unchanged."
            ),
        )?;
        let _ = pending.settle.send(ApprovalDecision::Approved);
        Ok(())
    }
}
