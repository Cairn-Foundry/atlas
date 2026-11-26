// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Launching an agent: its workflow, its folder and its first session.

use crate::language::tr;
use foundry_git::WorkspaceRequest;

use crate::contract::{CreateAgentInput, MessageKind, StepState};
use crate::error::{Result, refuse};
use crate::store::{AgentRecord, NewAgent};
use crate::text::to_camel_case;
use crate::workflow::{command_placeholders, workflow_issues};

use super::{AgentService, blocking};

const DEFAULT_BRANCH_TYPE: &str = "feat";

impl AgentService {
    fn new_branch_name(&self, input: &CreateAgentInput) -> String {
        let mut ticket = String::new();
        let mut is_in_invalid_run = false;
        for character in input.ticket_ref.trim().chars() {
            let is_valid = character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-');
            if is_valid {
                ticket.push(character);
            } else if !is_in_invalid_run {
                ticket.push('-');
            }
            is_in_invalid_run = !is_valid;
        }
        let name_source = input
            .branch_name
            .as_deref()
            .or(input.title.as_deref())
            .unwrap_or(&input.ticket_ref);
        self.0
            .deps
            .config
            .branch_pattern
            .replace("{type}", input.branch_type.as_deref().unwrap_or(DEFAULT_BRANCH_TYPE))
            .replace("{ticket}", &ticket)
            .replace("{name}", &to_camel_case(name_source))
    }

    pub async fn launch(&self, input: CreateAgentInput) -> Result<AgentRecord> {
        let repo = self.projects().resolve_for_launch(&input)?;
        let brick_ids = input
            .steps
            .clone()
            .unwrap_or_else(|| self.0.deps.config.default_steps.clone());
        let issues = workflow_issues(&self.catalogue(), &brick_ids, &[]);
        if !issues.is_empty() {
            refuse!("parcours refusé : {}", "workflow refused: {}", issues.join(" ; "));
        }
        let start_index = match &input.start_at {
            None => 0,
            Some(start_at) => match brick_ids.iter().position(|brick_id| brick_id == start_at) {
                Some(index) => index,
                None => refuse!(
                    "startAt ({start_at}) ne figure pas dans le parcours",
                    "startAt ({start_at}) is not in the workflow"
                ),
            },
        };
        let mut needs_mr_url = false;
        let mut has_committing_step = false;
        for brick_id in &brick_ids {
            let brick = &self.brick(brick_id)?;
            needs_mr_url |= command_placeholders(brick.command.as_deref().unwrap_or_default()).contains(&"mrUrl");
            has_committing_step |= brick.requires_clean_tree;
        }
        if needs_mr_url && input.mr_url.is_none() {
            refuse!(
                "ce parcours relit une merge request : donne son lien dans mrUrl",
                "this workflow reviews a merge request: give its link in mrUrl"
            );
        }

        let in_place = input.in_place.unwrap_or(false);
        let request = WorkspaceRequest {
            repo_path: repo.path.clone(),
            base_branch: repo.base_branch.clone(),
            new_worktree_path: self.new_worktree_path(&[&input.ticket_ref])?,
            new_branch: self.new_branch_name(&input),
            branch: input.branch.clone(),
            worktree_path: input.worktree_path.clone(),
            in_place,
            // Work that never commits (a review, an analysis) gets a folder, not a branch of its own.
            // So does work on a merge request whose branch is not known yet: the agent finds it and moves there.
            is_detached: input.branch.is_none()
                && input.worktree_path.is_none()
                && !in_place
                && (!has_committing_step || input.mr_url.is_some()),
        };
        let workspace = self
            .prepare_workspace(request, |reason| {
                tr!(
                    "préparation du dossier de travail impossible : {reason}",
                    "the working folder could not be prepared: {reason}"
                )
            })
            .await?;
        self.assert_folder_free(&workspace.path, None)?;
        self.trust(&workspace.repo_root);

        let mut steps = self.new_steps(&brick_ids, input.task_model.as_deref())?;
        for (index, step) in steps.iter_mut().enumerate() {
            match index.cmp(&start_index) {
                std::cmp::Ordering::Less => step.state = StepState::External,
                std::cmp::Ordering::Equal => step.state = StepState::Current,
                std::cmp::Ordering::Greater => {}
            }
        }
        let created = self.store().create_agent(&NewAgent {
            title: input.title.clone().unwrap_or_else(|| input.ticket_ref.clone()),
            ticket_ref: input.ticket_ref,
            repo_path: repo.path.clone(),
            worktree_path: workspace.path.clone(),
            branch: workspace.branch.clone(),
            base_branch: workspace.base_branch.clone(),
            note: input.note,
            owns_worktree: workspace.is_created,
            mr_url: input.mr_url,
            steps,
        });
        let agent = match created {
            Ok(agent) => agent,
            Err(error) => {
                // Nobody would ever find the folder Atlas just made for an agent that does not exist.
                if workspace.is_created {
                    let (repo_path, path, branch) =
                        (repo.path.clone(), workspace.path.clone(), workspace.branch.clone());
                    blocking(move || foundry_git::discard_workspace(&repo_path, &path, &branch)).await?;
                }
                return Err(error);
            }
        };
        self.post_card(
            MessageKind::Launch,
            &agent,
            input.reason.as_deref().unwrap_or(&tr!(
                "Lancé avec le parcours par défaut.",
                "Launched with the default workflow."
            )),
        )?;
        self.start_setup(
            &agent.id,
            self.projects().find_by_path(&repo.path)?.as_ref(),
            &workspace,
        )?;
        // A session that cannot open leaves the agent blocked, with the reason and a way to resume:
        // its folder and its workflow are ready, only Claude Code is missing.
        self.open_step_session_safely(&agent.id)?;
        self.store().require_agent(&agent.id)
    }
}
