// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The folders agents work in: worktrees prepared at launch, switched or added along the way.

use foundry_git::{Workspace, WorkspaceError, WorkspaceRequest};
use uuid::Uuid;

use crate::claude::trust_repository;
use crate::contract::{AgentStatus, MessageKind, StepState};
use crate::error::{Error, Result, refuse};
use crate::language::tr;
use crate::store::{AgentPatch, AgentRecord, ExtraWorkspace, StepPatch};
use crate::text::slugify;

use super::{AgentService, blocking};

const WORKTREE_SUFFIX_CHARS: usize = 6;

/// What git refused, in the language in service: the crate that finds the folders is shared with
/// other programs and words its errors once.
fn workspace_error_text(error: &WorkspaceError) -> String {
    match error {
        WorkspaceError::MainWorktreeDetached => tr!(
            "le dossier principal du projet n'est sur aucune branche",
            "the main folder of the project is on no branch"
        ),
        WorkspaceError::MainWorktreeOnOtherBranch { actual, wanted } => tr!(
            "le dossier principal est sur {actual}, pas sur {wanted}",
            "the main folder is on {actual}, not on {wanted}"
        ),
        WorkspaceError::NotAWorktree { path } => tr!(
            "{path} n'est pas un worktree de ce projet",
            "{path} is not a worktree of this project"
        ),
        WorkspaceError::WorktreeDetached { path } => tr!("{path} n'est sur aucune branche", "{path} is on no branch"),
        WorkspaceError::WorktreeOnOtherBranch { path, actual, wanted } => tr!(
            "{path} est sur {actual}, pas sur {wanted}",
            "{path} is on {actual}, not on {wanted}"
        ),
        WorkspaceError::WorktreeIsMain => tr!(
            "ce chemin est le dossier principal du projet",
            "this path is the main folder of the project"
        ),
        WorkspaceError::BranchOpenInMain { branch } => tr!(
            "{branch} est ouverte dans le dossier principal du projet",
            "{branch} is open in the main folder of the project"
        ),
        WorkspaceError::BranchNotFound { branch } => tr!(
            "la branche {branch} n'existe ni dans le dépôt ni sur origin",
            "the branch {branch} exists neither in the repository nor on origin"
        ),
        WorkspaceError::InvalidBranchName { branch } => {
            tr!("nom de branche invalide : {branch}", "invalid branch name: {branch}")
        }
        // A git command that failed, in git's own words, and whatever the crate adds later.
        _ => error.to_string(),
    }
}

/// A git refusal in the words of Atlas: what the master must ask for when the main folder is involved.
fn describe_workspace_error(error: &WorkspaceError) -> String {
    let what = workspace_error_text(error);
    match error {
        WorkspaceError::MainWorktreeOnOtherBranch { .. } => tr!(
            "{what} : Atlas n'y change pas de branche",
            "{what}: Atlas does not change branch there"
        ),
        WorkspaceError::WorktreeIsMain => tr!(
            "{what} : le maître doit le demander explicitement (inPlace)",
            "{what}: the master must explicitly ask for it (inPlace)"
        ),
        WorkspaceError::BranchOpenInMain { .. } => tr!(
            "{what} : le maître doit demander explicitement d'y travailler (inPlace)",
            "{what}: the master must explicitly ask to work there (inPlace)"
        ),
        _ => what,
    }
}

impl AgentService {
    pub(super) fn new_worktree_path(&self, name_parts: &[&str]) -> Result<String> {
        let dir = self.0.deps.home.join("worktrees");
        std::fs::create_dir_all(&dir)?;
        // As git will report it: two agents are kept apart by comparing these paths. Not on Windows,
        // where the resolved form is a verbatim path git and cmd.exe do not take.
        let dir = if cfg!(windows) {
            dir
        } else {
            std::fs::canonicalize(&dir)?
        };
        let suffix: String = Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(WORKTREE_SUFFIX_CHARS)
            .collect();
        let mut parts: Vec<String> = name_parts.iter().map(|part| slugify(part)).collect();
        parts.push(suffix);
        Ok(dir.join(parts.join("-")).to_string_lossy().into_owned())
    }

    /// Prepares a workspace; a git failure becomes a refusal worded by `describe_failure`.
    pub(super) async fn prepare_workspace(
        &self,
        request: WorkspaceRequest,
        describe_failure: impl FnOnce(&str) -> String,
    ) -> Result<Workspace> {
        blocking(move || foundry_git::prepare_workspace(&request))
            .await?
            .map_err(|error| Error::refusal(describe_failure(describe_workspace_error(&error).trim())))
    }

    /// Two agents never share a folder: refuses `path` when another active agent works there, as its main folder or a second one.
    pub(super) fn assert_folder_free(&self, path: &str, agent_id: Option<&str>) -> Result<()> {
        let occupant = self.store().list_agents(false)?.into_iter().find(|other| {
            Some(other.id.as_str()) != agent_id
                && other.status != AgentStatus::Done
                && (other.worktree_path == path
                    || other.workspaces.iter().any(|workspace| workspace.worktree_path == path))
        });
        if let Some(occupant) = occupant {
            refuse!(
                "{} travaille déjà dans {path} : deux agents ne partagent pas un dossier",
                "{} already works in {path}: two agents do not share a folder",
                occupant.ticket_ref
            );
        }
        Ok(())
    }

    pub(super) fn trust(&self, repo_root: &str) {
        if let Err(error) = trust_repository(repo_root, &self.0.deps.claude_state_path) {
            tracing::warn!(
                "{}",
                tr!(
                    "dépôt {repo_root} non marqué de confiance pour Claude Code : {error}",
                    "repository {repo_root} not marked as trusted for Claude Code: {error}"
                )
            );
        }
    }

    pub(super) async fn dirty_folders(&self, agent: &AgentRecord) -> Result<Vec<String>> {
        let folders: Vec<String> = std::iter::once(&agent.worktree_path)
            .chain(agent.workspaces.iter().map(|workspace| &workspace.worktree_path))
            .cloned()
            .collect();
        blocking(move || {
            folders
                .into_iter()
                .filter(|folder| !foundry_git::is_tree_clean(folder))
                .collect()
        })
        .await
    }

    /// Moves the agent onto an existing branch it found itself, typically the source branch of the
    /// merge request it must fix: nobody could name that branch when the agent was launched. The
    /// folder it leaves is removed if Atlas made it and nothing was done there.
    pub async fn work_on_branch(&self, agent_id: &str, branch: &str, base_branch: Option<&str>) -> Result<String> {
        let agent = self.store().require_agent(agent_id)?;
        self.require_current(&agent)?;
        if agent.branch == branch {
            return Ok(format!("You are already on {branch}, in {}.", agent.worktree_path));
        }
        if !agent.workspaces.is_empty() {
            refuse!(
                "impossible de changer de branche une fois un second dépôt ouvert",
                "the branch cannot change once a second repository is open"
            );
        }
        let project = self.projects().find_by_path(&agent.repo_path)?;
        let request = WorkspaceRequest {
            repo_path: agent.repo_path.clone(),
            base_branch: base_branch
                .map(str::to_owned)
                .or_else(|| project.as_ref().and_then(|repo| repo.base_branch.clone())),
            new_worktree_path: self.new_worktree_path(&[&agent.ticket_ref])?,
            new_branch: branch.to_owned(),
            branch: Some(branch.to_owned()),
            ..WorkspaceRequest::default()
        };
        let workspace = self
            .prepare_workspace(request, |reason| {
                tr!("impossible de s'installer sur {branch} : {reason}. work_on_branch ne prend qu'une branche qui existe déjà.", "could not settle on {branch}: {reason}. work_on_branch only takes a branch that already exists.")
            })
            .await?;
        self.assert_folder_free(&workspace.path, Some(agent_id))?;

        if agent.owns_worktree {
            let (repo_path, path, left_branch) = (
                agent.repo_path.clone(),
                agent.worktree_path.clone(),
                agent.branch.clone(),
            );
            blocking(move || foundry_git::discard_workspace(&repo_path, &path, &left_branch)).await?;
        }
        // The sessions of the folder it leaves cannot be resumed from another one.
        self.store().retire_sessions(agent_id)?;
        for step in agent
            .steps
            .iter()
            .filter(|candidate| candidate.state == StepState::Current)
        {
            self.store().update_step(
                &step.id,
                StepPatch {
                    session_id: Some(None),
                    ..StepPatch::default()
                },
            )?;
        }
        let moved = self.update(
            agent_id,
            AgentPatch {
                worktree_path: Some(workspace.path.clone()),
                branch: Some(workspace.branch.clone()),
                base_branch: Some(base_branch.map_or_else(|| workspace.base_branch.clone(), str::to_owned)),
                owns_worktree: Some(workspace.is_created),
                stop_blocks: Some(0),
                ..AgentPatch::default()
            },
        )?;
        self.post_card(
            MessageKind::Decision,
            &moved,
            &tr!(
                "L'agent s'installe sur la branche {branch}, qu'il a trouvée lui-même.",
                "The agent settles on the branch {branch}, which it found itself."
            ),
        )?;
        self.start_setup(agent_id, project.as_ref(), &workspace)?;
        self.move_to_new_session(agent_id, &format!("moved to {branch}"))?;
        Ok(format!(
            "Atlas settles you in {}, on {branch}. This session is about to close and the step resumes there in a new session: end without any other action.",
            workspace.path
        ))
    }

    /// Opens a second project to an agent whose work spans two repositories: a worktree on the same
    /// branch name, so the review reads both and the ship opens one merge request per project.
    pub async fn request_repo(&self, agent_id: &str, repo_name: &str, reason: &str) -> Result<String> {
        let agent = self.store().require_agent(agent_id)?;
        self.require_not_delivered(&agent)?;
        let Some(repo) = self.projects().find_by_reference(repo_name)? else {
            self.warn_master(
                &tr!("L'agent qui travaille sur {} a besoin d'un autre dépôt, {repo_name}, qui n'est pas un projet déclaré. Donnez son chemin à l'orchestrateur pour l'ajouter.", "The agent working on {} needs another repository, {repo_name}, which is not a declared project. Give its path to the orchestrator to add it.",
                    agent.ticket_ref
                ),
                Some(agent_id),
            )?;
            let names: Vec<String> = self
                .projects()
                .list()?
                .into_iter()
                .map(|candidate| candidate.name)
                .collect();
            refuse!(
                "projet inconnu : {repo_name}. Projets déclarés : {}. Le maître est prévenu : appelle ask_user, puis rappelle request_repo quand il a ajouté le projet.",
                "unknown project: {repo_name}. Declared projects: {}. The master is told: call ask_user, then call request_repo again once he has added the project.",
                if names.is_empty() {
                    tr!("aucun", "none")
                } else {
                    names.join(", ")
                }
            );
        };
        if agent.branch.is_empty() {
            refuse!(
                "tu n'as pas de branche : appelle d'abord work_on_branch si ce travail doit modifier du code",
                "you have no branch: call work_on_branch first if this work must change code"
            );
        }
        let known = if repo.path == agent.repo_path {
            Some(&agent.worktree_path)
        } else {
            agent
                .workspaces
                .iter()
                .find(|workspace| workspace.repo_path == repo.path)
                .map(|workspace| &workspace.worktree_path)
        };
        if let Some(known) = known {
            return Ok(format!("You already work in this project: {known}."));
        }

        let request = WorkspaceRequest {
            repo_path: repo.path.clone(),
            base_branch: repo.base_branch.clone(),
            new_worktree_path: self.new_worktree_path(&[&agent.ticket_ref, &repo.name])?,
            new_branch: agent.branch.clone(),
            ..WorkspaceRequest::default()
        };
        let workspace = self
            .prepare_workspace(request, |failure| {
                tr!(
                    "préparation du dossier dans {} impossible : {failure}",
                    "the folder in {} could not be prepared: {failure}",
                    repo.name
                )
            })
            .await?;
        self.assert_folder_free(&workspace.path, Some(agent_id))?;
        self.trust(&workspace.repo_root);
        self.store().add_workspace(
            agent_id,
            &ExtraWorkspace {
                repo_name: repo.name.clone(),
                repo_path: repo.path.clone(),
                worktree_path: workspace.path.clone(),
                branch: workspace.branch.clone(),
                base_branch: workspace.base_branch.clone(),
            },
        )?;
        let updated = self.update(agent_id, AgentPatch::default())?;
        self.post_card(
            MessageKind::Decision,
            &updated,
            &tr!(
                "Dépôt {} ouvert à l'agent, à sa demande : {reason}",
                "Repository {} opened to the agent, at its request: {reason}",
                repo.name
            ),
        )?;
        let setup = self.start_setup(agent_id, Some(&repo), &workspace)?;
        let mut sentences = vec![format!(
            "Folder ready: {}, branch {} (base {}).",
            workspace.path, workspace.branch, workspace.base_branch
        )];
        if let Some(setup) = setup {
            sentences.push(format!(
                "Atlas is installing the dependencies there ({}), log in {}: do not install them yourself, wait for the end of this log before building.",
                setup.command, setup.log_path
            ));
        }
        sentences.push(
            "Work there as in your main folder: same rules, commits before closing the step, nothing pushed before the Ship.".to_owned(),
        );
        sentences.push(
            "The review will read both repositories together and the Ship will open one merge request per repository."
                .to_owned(),
        );
        Ok(sentences.join(" "))
    }
}
