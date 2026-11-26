// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Moving an agent through its workflow: the brief of a step, its completion, the review passes
//! and the changes the orchestrator or the master make to what remains.

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;

use crate::brick::{Brick, BrickApproval};
use crate::claude::read_usage;
use crate::clock::iso;
use crate::contract::{AgentStatus, MessageAction, MessageKind, StepOutput, StepState};
use crate::error::{Error, Result, refuse};
use crate::language::tr;
use crate::prompts::{NO_BRANCH_NOTICE, step_brief};
use crate::store::{AgentPatch, AgentRecord, NewStep, StepPatch, StepRecord};
use crate::workflow::{
    LoopingReview, ReviewOutcome, SessionTraits, can_share_session, evaluate_review_pass, looping_review, pass_model,
    workflow_issues,
};

use super::{AgentService, ApprovalDecision, SetupRun, blocking};

/// Longest a step that builds waits for an install before it starts anyway.
const SETUP_WAIT: Duration = Duration::from_secs(15 * 60);
/// The free task brick: the only one whose model the orchestrator picks at launch.
const TASK_BRICK_ID: &str = "tache";

/// A field of `complete_step` as an agent hands it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum FieldValue {
    Text(String),
    Number(serde_json::Number),
    List(Vec<String>),
}

/// A field as text: agents often hand a list where a text is asked, which is no reason to refuse the step.
pub(super) fn field_text(value: Option<&FieldValue>) -> String {
    match value {
        None => String::new(),
        Some(FieldValue::Text(text)) => text.trim().to_owned(),
        Some(FieldValue::Number(number)) => number.to_string(),
        Some(FieldValue::List(items)) if items.is_empty() => "none".to_owned(),
        Some(FieldValue::List(items)) => items
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteStepInput {
    /// The fields the brick asks for, by name; a list is taken as one item per line.
    pub fields: HashMap<String, FieldValue>,
    #[serde(default)]
    pub findings_above_threshold: Option<u32>,
}

fn plural(count: u32) -> &'static str {
    if count > 1 { "s" } else { "" }
}

impl AgentService {
    pub(super) fn new_steps(&self, brick_ids: &[String], task_model: Option<&str>) -> Result<Vec<NewStep>> {
        brick_ids
            .iter()
            .map(|brick_id| {
                let brick = &self.brick(brick_id)?;
                let model = task_model.filter(|_| brick_id == TASK_BRICK_ID).unwrap_or(&brick.model);
                Ok(NewStep {
                    brick_id: brick_id.clone(),
                    name: brick.name.clone(),
                    model: model.to_owned(),
                    state: StepState::Upcoming,
                    pass: 1,
                })
            })
            .collect()
    }

    pub fn next_step(&self, agent_id: &str) -> Result<String> {
        let agent = self.store().require_agent(agent_id)?;
        let Some(step) = agent.current_step().filter(|_| agent.status != AgentStatus::Done) else {
            return Ok("The work is delivered: no step left. Answer the master if he asks you a question.".to_owned());
        };
        let brick = &self.brick(&step.brick_id)?;
        let mut parts = vec![step_brief(&agent, step, brick)];
        if brick.needs_dependencies {
            parts.extend(self.setup_notices(agent_id));
        }
        self.update(
            agent_id,
            AgentPatch {
                stop_blocks: Some(0),
                ..AgentPatch::default()
            },
        )?;
        Ok(parts.join("\n\n"))
    }

    /// `next_step` as the agent calls it: a step that builds first waits for the installs Atlas runs in its folders.
    pub async fn next_step_when_ready(&self, agent_id: &str) -> Result<String> {
        let agent = self.store().require_agent(agent_id)?;
        let needs_dependencies = match agent.current_step() {
            Some(step) => self.brick(&step.brick_id)?.needs_dependencies,
            None => false,
        };
        let running: Vec<SetupRun> = self
            .setup_runs(agent_id)
            .into_iter()
            .filter(|run| run.result.borrow().is_none())
            .collect();
        if needs_dependencies && !running.is_empty() {
            let all_done = async {
                for mut run in running {
                    // A sender dropped without a result means the install task is gone: nothing left to wait for.
                    let _ = run.result.wait_for(Option::is_some).await;
                }
            };
            let _ = tokio::time::timeout(SETUP_WAIT, all_done).await;
        }
        self.next_step(agent_id)
    }

    fn read_fields(&self, brick: &Brick, fields: &HashMap<String, FieldValue>) -> Result<StepOutput> {
        let mut output = StepOutput::new();
        let mut missing = Vec::new();
        for field in brick.writes.keys() {
            let value = field_text(fields.get(field));
            if value.is_empty() {
                missing.push(field.as_str());
            } else {
                output.insert(field.clone(), value);
            }
        }
        if !missing.is_empty() {
            refuse!(
                "champs manquants dans fields : {}. Une liste vide s'écrit \"none\".",
                "missing fields in fields: {}. An empty list is written \"none\".",
                missing.join(", ")
            );
        }
        Ok(output)
    }

    pub async fn complete_step(&self, agent_id: &str, input: CompleteStepInput) -> Result<String> {
        let agent = self.store().require_agent(agent_id)?;
        self.require_not_delivered(&agent)?;
        if agent.status == AgentStatus::Waiting {
            refuse!(
                "tu as posé une question au maître avec ask_user : attends sa réponse dans ce terminal avant de clore l'étape",
                "you asked the master a question with ask_user: wait for his answer in this terminal before closing the step"
            );
        }
        let step = self.require_current(&agent)?.clone();
        let brick = &self.brick(&step.brick_id)?;
        if brick.approval == BrickApproval::Master && !agent.approval_requested {
            refuse!(
                "l'étape {} demande la validation du maître : soumets-la avec ask_user et attends sa réponse",
                "the step {} requires the validation of the master: submit it with ask_user and wait for his answer",
                step.name
            );
        }
        let output = self.read_fields(brick, &input.fields)?;
        if brick.requires_clean_tree {
            if agent.branch.is_empty() {
                return Err(Error::refusal(NO_BRANCH_NOTICE));
            }
            let dirty = self.dirty_folders(&agent).await?;
            if !dirty.is_empty() {
                refuse!(
                    "git status n'est pas propre dans {} : commite ton travail (commits atomiques, au format du dépôt) avant de clore l'étape. \
                     Les étapes suivantes lisent des commits. Atlas prime ici sur une règle du dépôt qui interdit de commiter.",
                    "git status is not clean in {}: commit your work (atomic commits, in the format of the repository) before closing the step. \
                     The next steps read commits. Atlas prevails here over a rule of the repository that forbids committing.",
                    dirty.join(", ")
                );
            }
        }
        let findings = match (looping_review(brick), input.findings_above_threshold) {
            (None, _) => None,
            (Some(_), Some(findings)) => Some(findings),
            (Some(_), None) => {
                refuse!(
                    "indique findingsAboveThreshold : le nombre de findings au seuil trouvés pendant cette passe",
                    "give findingsAboveThreshold: the number of findings at the threshold found during this pass"
                )
            }
        };
        if brick.approval == BrickApproval::Orchestrator
            && let ApprovalDecision::Rejected(reason) = self.request_approval(&agent, &step, &output).await?
        {
            refuse!(
                "l'orchestrateur renvoie l'étape {} : {reason}. Complète-la, puis rappelle complete_step.",
                "the orchestrator sends the step {} back: {reason}. Complete it, then call complete_step again.",
                step.name
            );
        }
        self.close_step(agent_id, &step.id, output, findings).await
    }

    /// Records what the current step produced and cost, then moves the agent on.
    async fn close_step(
        &self,
        agent_id: &str,
        step_id: &str,
        output: StepOutput,
        findings: Option<u32>,
    ) -> Result<String> {
        let find_step = |agent: &AgentRecord| agent.steps.iter().find(|candidate| candidate.id == step_id).cloned();
        let unknown_step = || Error::internal(tr!("étape inconnue : {step_id}", "unknown step: {step_id}"));
        let asked = self.store().require_agent(agent_id)?;
        let session = find_step(&asked).ok_or_else(unknown_step)?;
        let ended_at = iso(self.now());
        let (worktree, claude_dir, until) = (asked.worktree_path, self.0.deps.claude_dir.clone(), ended_at.clone());
        // The transcripts of a long session weigh tens of megabytes: read off the runtime thread.
        let (head_sha, usage) = blocking(move || {
            let usage = match (&session.session_id, &session.started_at) {
                (Some(session_id), Some(started_at)) => read_usage(&claude_dir, session_id, started_at, &until),
                _ => None,
            };
            (foundry_git::head_commit(&worktree), usage)
        })
        .await?;
        // Nothing awaits from here on: of two calls for the same step, the second finds it closed.
        let before = self.store().require_agent(agent_id)?;
        let step = find_step(&before).ok_or_else(unknown_step)?;
        if step.state != StepState::Current {
            refuse!(
                "l'étape {} est déjà close : appelle next_step pour la suivante",
                "the step {} is already closed: call next_step for the next one",
                step.name
            );
        }
        // Every brick the move needs is found before the first write: one the master deleted
        // meanwhile refuses the call instead of leaving the agent between two steps.
        let brick = &self.brick(&step.brick_id)?;
        for later in before.upcoming_steps() {
            self.brick(&later.brick_id)?;
        }
        let opened_mr = output.get("mrUrl").filter(|url| !url.is_empty()).cloned();
        self.store().update_step(
            &step.id,
            StepPatch {
                state: Some(StepState::Done),
                ended_at: Some(Some(ended_at)),
                wait_ms: Some(before.step_wait_ms),
                output: Some(Some(output)),
                findings: Some(findings),
                head_sha: Some(head_sha),
                usage: Some(usage),
                ..StepPatch::default()
            },
        )?;
        if let Some(mr_url) = opened_mr {
            self.update(
                agent_id,
                AgentPatch {
                    mr_url: Some(Some(mr_url)),
                    ..AgentPatch::default()
                },
            )?;
        }

        let hold_reason = match (looping_review(brick), findings) {
            (Some(review), Some(findings)) => self.apply_review_outcome(agent_id, &step, brick, review, findings)?,
            _ => None,
        };
        self.enter_next_step(agent_id, &step, hold_reason)
    }

    /// Shapes the steps that follow a finished review pass. Returns why the agent must stop for the
    /// master, none when it carries on.
    fn apply_review_outcome(
        &self,
        agent_id: &str,
        step: &StepRecord,
        brick: &Brick,
        review: LoopingReview<'_>,
        findings: u32,
    ) -> Result<Option<String>> {
        let agent = self.store().require_agent(agent_id)?;
        let upcoming: Vec<&StepRecord> = agent.upcoming_steps().collect();
        let next = upcoming.first().copied();
        let is_fix_planned = next.is_some_and(|next| next.brick_id == review.fixed_by);
        let mut is_needed_later = false;
        for later in upcoming.iter().filter(|later| later.brick_id != review.fixed_by) {
            is_needed_later |= &self.brick(&later.brick_id)?.requires.contains(&brick.id);
        }
        match evaluate_review_pass(
            review,
            step.pass,
            findings,
            is_fix_planned || step.pass > 1 || is_needed_later,
        ) {
            ReviewOutcome::Converged => {
                let Some(next) = next.filter(|_| is_fix_planned) else {
                    return Ok(None);
                };
                let skipped = || StepPatch {
                    state: Some(StepState::Skipped),
                    ..StepPatch::default()
                };
                self.store().update_step(&next.id, skipped())?;
                // The pass planned to reread the correction would reread the same commits.
                if let Some(recheck) = upcoming.get(1).filter(|recheck| recheck.brick_id == brick.id) {
                    self.store().update_step(&recheck.id, skipped())?;
                }
                Ok(None)
            }
            ReviewOutcome::ReportOnly => Ok(None),
            ReviewOutcome::FixAndReview => {
                if !is_fix_planned
                    && let Some(fix) = self.new_steps(&[review.fixed_by.to_owned()], None)?.into_iter().next()
                {
                    self.store().insert_step_after(agent_id, &step.id, &fix)?;
                }
                let refreshed = self.store().require_agent(agent_id)?;
                if let Some(fix_step) = refreshed.upcoming_steps().next() {
                    let pass = step.pass + 1;
                    let next_pass = NewStep {
                        brick_id: step.brick_id.clone(),
                        name: step.name.clone(),
                        model: pass_model(brick, pass).to_owned(),
                        state: StepState::Upcoming,
                        pass,
                    };
                    self.store().insert_step_after(agent_id, &fix_step.id, &next_pass)?;
                }
                Ok(None)
            }
            ReviewOutcome::Exhausted => Ok(Some(tr!(
                "{} passes de review sans converger : il reste {findings} finding{} au seuil",
                "{} review passes without converging: {findings} finding{} left at the threshold",
                step.pass,
                plural(findings)
            ))),
        }
    }

    /// What an agent entering a step starts from: clocks reset, flags of the previous step cleared.
    fn step_start_patch(started_at: &str) -> AgentPatch {
        AgentPatch {
            step_started_at: Some(started_at.to_owned()),
            step_wait_ms: Some(0),
            waiting_since: Some(None),
            approval_requested: Some(false),
            threshold_alerted: Some(false),
            stop_blocks: Some(0),
            status: Some(AgentStatus::Working),
            status_reason: Some(None),
            ..AgentPatch::default()
        }
    }

    /// Moves the agent to its next step. The live session carries on when the next brick shares it;
    /// otherwise it closes and the next step opens its own. With `hold_reason` the agent stops before
    /// the next step and the master is asked.
    fn enter_next_step(&self, agent_id: &str, closed: &StepRecord, hold_reason: Option<String>) -> Result<String> {
        let agent = self.store().require_agent(agent_id)?;
        let started_at = iso(self.now());
        let start = Self::step_start_patch(&started_at);
        let close_delay = self.0.deps.close_delay;

        let Some(next) = agent.upcoming_steps().next() else {
            self.close_session(agent_id, "work delivered", close_delay)?;
            let done = self.update(
                agent_id,
                AgentPatch {
                    status: Some(AgentStatus::Done),
                    ..start
                },
            )?;
            // A merge request the agent was given to read is not one it opened.
            let has_opened_mr = agent
                .steps
                .iter()
                .any(|step| step.output.as_ref().is_some_and(|output| output.contains_key("mrUrl")));
            let what = if has_opened_mr {
                tr!("La MR de {} est ouverte", "The MR of {} is open", agent.ticket_ref)
            } else {
                tr!("{} est terminé", "{} is done", agent.ticket_ref)
            };
            let open = match done.mr_url {
                Some(url) => MessageAction::Link {
                    label: tr!("Ouvrir la MR", "Open the MR"),
                    url,
                },
                None => MessageAction::Session,
            };
            let text = match hold_reason {
                Some(reason) => tr!("{what}, mais {reason}.", "{what}, but {reason}."),
                None => tr!("{what} et attend votre relecture.", "{what} and waits for your review."),
            };
            self.warn_master_with(&text, Some(agent_id), vec![open, MessageAction::Archive])?;
            return Ok("Work delivered. This session is about to close; its id is kept for a resume.".to_owned());
        };

        self.store().update_step(
            &next.id,
            StepPatch {
                state: Some(StepState::Current),
                started_at: Some(Some(started_at.clone())),
                ..StepPatch::default()
            },
        )?;
        let on_next_step = AgentPatch {
            step_id: Some(next.brick_id.clone()),
            ..start
        };
        if let Some(reason) = hold_reason {
            self.close_session(agent_id, &format!("held before {}", next.brick_id), close_delay)?;
            self.update(
                agent_id,
                AgentPatch {
                    status: Some(AgentStatus::Blocked),
                    status_reason: Some(Some(reason.clone())),
                    // The hold is a wait for the master, not time spent on the step.
                    waiting_since: Some(Some(started_at)),
                    ..on_next_step
                },
            )?;
            self.warn_master(
                &tr!(
                    "{} : {reason}. Que voulez-vous faire ?",
                    "{}: {reason}. What do you want to do?",
                    agent.ticket_ref
                ),
                Some(agent_id),
            )?;
            return Ok(
                "Step closed. The master is told and will decide what comes next: this session is about to close, end without any other action.".to_owned()
            );
        }

        let next_brick = &self.brick(&next.brick_id)?;
        let shared = self.live(agent_id).filter(|live| {
            let session = SessionTraits {
                group: &live.group,
                model: &live.model,
                has_subagents: live.has_subagents,
                skills: &live.skills,
                subagent_types: &live.subagent_types,
            };
            can_share_session(session, next_brick, &next.model)
        });
        self.update(agent_id, on_next_step)?;
        if let Some(live) = shared {
            self.store().update_step(
                &next.id,
                StepPatch {
                    session_id: Some(Some(live.session_id.clone())),
                    ..StepPatch::default()
                },
            )?;
            // The next step's brief comes with the reply: one call and one turn less than asking for it.
            return Ok(format!(
                "Step {} closed. Go straight on to the next one, in this session.\n\n{}",
                closed.name,
                self.next_step(agent_id)?
            ));
        }
        self.move_to_new_session(agent_id, &format!("step {} done", closed.brick_id))?;
        Ok(format!(
            "Step {} closed. The step {} goes to another session: this one is about to close, end without any other action.",
            closed.name, next.name
        ))
    }

    /// Replaces the steps the agent has not started with `brick_ids`. A current step that has no
    /// session yet (the agent was held before it) counts as not started. Steps that disappear stay
    /// in the history as skipped. Returns true when a new current step needs its session opened.
    pub(super) fn replace_upcoming(&self, agent_id: &str, brick_ids: &[String]) -> Result<bool> {
        let agent = self.store().require_agent(agent_id)?;
        let current = agent.current_step();
        let is_current_started =
            current.is_some_and(|step| step.session_id.is_some() || self.has_live_session(agent_id));
        let is_current = |step: &StepRecord| current.is_some_and(|current| current.id == step.id);
        let kept: Vec<&StepRecord> = agent
            .steps
            .iter()
            .filter(|step| step.is_finished() || (is_current(step) && is_current_started))
            .collect();
        let kept_ids: Vec<String> = kept.iter().map(|step| step.brick_id.clone()).collect();
        let issues = workflow_issues(&self.catalogue(), brick_ids, &kept_ids);
        if !issues.is_empty() {
            refuse!("parcours refusé : {}", "workflow refused: {}", issues.join(" ; "));
        }
        let mut remaining: Vec<&String> = brick_ids.iter().collect();
        let dropped: Vec<NewStep> = agent
            .steps
            .iter()
            .filter(|step| step.state == StepState::Upcoming || (is_current(step) && !is_current_started))
            .filter(
                |step| match remaining.iter().position(|brick_id| **brick_id == step.brick_id) {
                    Some(index) => {
                        remaining.remove(index);
                        false
                    }
                    None => true,
                },
            )
            .map(|step| NewStep {
                brick_id: step.brick_id.clone(),
                name: step.name.clone(),
                model: step.model.clone(),
                state: StepState::Skipped,
                pass: step.pass,
            })
            .collect();
        if let Some(current) = current.filter(|_| !is_current_started) {
            self.store().update_step(
                &current.id,
                StepPatch {
                    state: Some(StepState::Upcoming),
                    ..StepPatch::default()
                },
            )?;
        }
        let upcoming = self.continue_passes(&kept, self.new_steps(brick_ids, None)?)?;
        self.store().replace_upcoming_steps(agent_id, &dropped, &upcoming)?;
        if is_current_started {
            return Ok(false);
        }

        let refreshed = self.store().require_agent(agent_id)?;
        let Some(first) = refreshed.upcoming_steps().next() else {
            if agent.status != AgentStatus::Done {
                self.update(
                    agent_id,
                    AgentPatch {
                        status: Some(AgentStatus::Done),
                        status_reason: Some(None),
                        ..AgentPatch::default()
                    },
                )?;
            }
            return Ok(false);
        };
        let started_at = iso(self.now());
        self.store().update_step(
            &first.id,
            StepPatch {
                state: Some(StepState::Current),
                started_at: Some(Some(started_at.clone())),
                ..StepPatch::default()
            },
        )?;
        self.update(
            agent_id,
            AgentPatch {
                step_id: Some(first.brick_id.clone()),
                ..Self::step_start_patch(&started_at)
            },
        )?;
        Ok(true)
    }

    /// Numbers the review passes of `steps` after those the agent already ran: a pass the master asks
    /// for once the reviews stopped is the next one, so it rereads only the fix commits and the cap
    /// still holds.
    fn continue_passes(&self, history: &[&StepRecord], mut steps: Vec<NewStep>) -> Result<Vec<NewStep>> {
        let mut last_pass: HashMap<String, u32> = HashMap::new();
        for step in history.iter().filter(|step| step.state == StepState::Done) {
            let pass = last_pass.entry(step.brick_id.clone()).or_default();
            *pass = (*pass).max(step.pass);
        }
        for step in &mut steps {
            let brick = &self.brick(&step.brick_id)?;
            if looping_review(brick).is_none() {
                continue;
            }
            let pass = last_pass.entry(step.brick_id.clone()).or_default();
            *pass += 1;
            step.pass = *pass;
            step.model = pass_model(brick, *pass).to_owned();
        }
        Ok(steps)
    }

    /// The orchestrator or the master changes what remains of an agent's workflow.
    pub fn set_workflow(&self, agent_id: &str, brick_ids: &[String], reason: &str) -> Result<AgentRecord> {
        let needs_session = self.replace_upcoming(agent_id, brick_ids)?;
        self.post_card(MessageKind::Workflow, &self.store().require_agent(agent_id)?, reason)?;
        if needs_session && !self.has_live_session(agent_id) {
            return self.open_step_session_or_block(agent_id, false, None);
        }
        self.update(agent_id, AgentPatch::default())
    }

    pub fn ask_user(&self, agent_id: &str, question: &str) -> Result<String> {
        let agent = self.store().require_agent(agent_id)?;
        let step = self.require_current(&agent)?;
        let needs_master = self.brick(&step.brick_id)?.approval == BrickApproval::Master;
        self.update(
            agent_id,
            AgentPatch {
                status: Some(AgentStatus::Waiting),
                status_reason: Some(Some(question.to_owned())),
                waiting_since: Some(Some(agent.waiting_since.clone().unwrap_or_else(|| iso(self.now())))),
                approval_requested: Some(agent.approval_requested || needs_master),
                stop_blocks: Some(0),
                ..AgentPatch::default()
            },
        )?;
        if agent.status != AgentStatus::Waiting {
            self.warn_master(
                &tr!(
                    "L'agent qui travaille sur {} a besoin de votre aide.",
                    "The agent working on {} needs your help.",
                    agent.ticket_ref
                ),
                Some(agent_id),
            )?;
        }
        Ok([
            "The master is told, but he does not see the text passed to ask_user: now write your complete question in this terminal",
            "(the useful context, the options, your recommendation), then end your turn and wait for his answer here.",
            "Do nothing that depends on his answer and do not call complete_step before he has answered.",
        ]
        .join(" "))
    }

    pub fn block(&self, agent_id: &str, reason: &str) -> Result<String> {
        let agent = self.store().require_agent(agent_id)?;
        self.require_not_delivered(&agent)?;
        self.update(
            agent_id,
            AgentPatch {
                status: Some(AgentStatus::Blocked),
                status_reason: Some(Some(reason.to_owned())),
                waiting_since: Some(Some(agent.waiting_since.clone().unwrap_or_else(|| iso(self.now())))),
                stop_blocks: Some(0),
                ..AgentPatch::default()
            },
        )?;
        self.warn_master(
            &tr!(
                "L'agent qui travaille sur {} est bloqué et a besoin de votre aide.",
                "The agent working on {} is blocked and needs your help.",
                agent.ticket_ref
            ),
            Some(agent_id),
        )?;
        Ok("The master is told. Write in this terminal what blocks you, what you tried and what you need, then end your turn and wait for his answer here.".to_owned())
    }

    /// Gives up the current step and sends the agent on with `brick_ids`, in a fresh session: the
    /// master wants it to ship now, or to go back to a correction. The given-up step stays in the
    /// history as skipped.
    pub fn jump_to(&self, agent_id: &str, brick_ids: &[String], reason: &str) -> Result<AgentRecord> {
        let agent = self.store().require_agent(agent_id)?;
        if self.has_pending_approval(agent_id) {
            refuse!(
                "une étape de cet agent attend ta validation : décide d'abord avec approve_step",
                "a step of this agent waits for your validation: decide first with approve_step"
            );
        }
        let finished: Vec<String> = agent
            .steps
            .iter()
            .filter(|step| step.is_finished())
            .map(|step| step.brick_id.clone())
            .collect();
        let issues = workflow_issues(&self.catalogue(), brick_ids, &finished);
        if !issues.is_empty() {
            refuse!("parcours refusé : {}", "workflow refused: {}", issues.join(" ; "));
        }
        self.close_session(agent_id, "moved on by the orchestrator", Duration::ZERO)?;
        if let Some(current) = agent.current_step() {
            self.store().update_step(
                &current.id,
                StepPatch {
                    state: Some(StepState::Skipped),
                    ended_at: Some(Some(iso(self.now()))),
                    ..StepPatch::default()
                },
            )?;
        }
        let needs_session = self.replace_upcoming(agent_id, brick_ids)?;
        self.post_card(MessageKind::Workflow, &self.store().require_agent(agent_id)?, reason)?;
        if needs_session {
            self.open_step_session_or_block(agent_id, false, None)
        } else {
            self.update(agent_id, AgentPatch::default())
        }
    }
}
