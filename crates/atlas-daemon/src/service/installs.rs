// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Dependency installs Atlas runs in the worktrees it creates.

use crate::language::tr;
use std::sync::PoisonError;

use foundry_git::Workspace;
use tokio::sync::watch;

use crate::contract::RepoView;
use crate::error::Result;
use crate::setup::run_setup;
use crate::store::MessageExtras;

use super::{AgentService, SetupRun};

impl AgentService {
    pub(super) fn setup_runs(&self, agent_id: &str) -> Vec<SetupRun> {
        self.0
            .setups
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(agent_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Installs the project's dependencies in a worktree Atlas just created, in the background: the
    /// agent starts on what needs no build (qualification, plan) meanwhile. A folder that already
    /// existed has its dependencies already, or the master's own.
    pub(super) fn start_setup(
        &self,
        agent_id: &str,
        repo: Option<&RepoView>,
        workspace: &Workspace,
    ) -> Result<Option<SetupRun>> {
        let Some(command) = repo
            .and_then(|repo| repo.setup.clone())
            .filter(|_| workspace.is_created)
        else {
            return Ok(None);
        };
        let dir = self.agent_dir(agent_id)?;
        let run_number = self.setup_runs(agent_id).len() + 1;
        let log_path = dir
            .join(format!("setup-{run_number}.log"))
            .to_string_lossy()
            .into_owned();
        let (done, result) = watch::channel(None);
        let run = SetupRun {
            folder: workspace.path.clone(),
            command,
            log_path,
            result,
        };
        self.0
            .setups
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(agent_id.to_owned())
            .or_default()
            .push(run.clone());

        let service = self.clone();
        let agent_id = agent_id.to_owned();
        let started = run.clone();
        tokio::spawn(async move {
            let result = run_setup(&started.command, &started.folder, &started.log_path).await;
            let _ = done.send(Some(result));
            if result.is_success() {
                return;
            }
            let Ok(Some(agent)) = service.store().get_agent(&agent_id) else {
                return;
            };
            let text = tr!(
                "L'installation des dépendances de {} a échoué ({}) : voir {}. L'agent les installera lui-même.",
                "Installing the dependencies of {} failed ({}): see {}. The agent will install them itself.",
                agent.ticket_ref,
                started.command,
                started.log_path
            );
            if let Err(error) = service.post_message(&text, Some(&agent_id), MessageExtras::default()) {
                tracing::error!(
                    "{}",
                    tr!(
                        "message d'échec d'installation : {error}",
                        "install failure message: {error}"
                    )
                );
            }
        });
        Ok(Some(run))
    }

    /// What a step that builds must know about the installs: failed, or still running past the wait.
    pub(super) fn setup_notices(&self, agent_id: &str) -> Vec<String> {
        self.setup_runs(agent_id)
            .iter()
            .filter_map(|run| match *run.result.borrow() {
                None => Some(format!(
                    "Atlas is still installing the dependencies in {} ({}), log in {}: do not run it again, wait for it to end before building.",
                    run.folder, run.command, run.log_path
                )),
                Some(result) if !result.is_success() => Some(format!(
                    "Installing the dependencies failed in {} ({}), log in {}: read it, then install them yourself.",
                    run.folder, run.command, run.log_path
                )),
                Some(_) => None,
            })
            .collect()
    }
}
