// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! `~/.atlas/config.yml`: what the master tunes.

use crate::language::{self, Language, tr};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::contract::{Issue, RepoInput};

const DEFAULT_CONFIG_YAML_FR: &str = r#"# Configuration d'Atlas
port: 4317
# Modèle de l'orchestrateur (claude -p)
orchestratorModel: sonnet
# Commande qui lance Claude Code
claudeCommand: claude
# Arguments ajoutés à chaque session d'agent. Les agents tournent déjà en bypassPermissions.
agentArgs: []
# Dépôts dans lesquels l'orchestrateur peut lancer des agents
repos: []
#  - name: atlas
#    path: /chemin/vers/le/depot
#    setup: pnpm install --frozen-lockfile   # lancée dans chaque worktree neuf
supervisorIntervalSeconds: 30
# Parcours d'un agent lancé sans parcours précis (briques de ~/.atlas/bricks)
defaultSteps: [qualification, plan, implementation, tests, review, correction, ship]
# Nom d'une branche neuve
branchPattern: "{type}/{ticket}/{name}"
"#;

const DEFAULT_CONFIG_YAML_EN: &str = r#"# Configuration of Atlas
port: 4317
# Model of the orchestrator (claude -p)
orchestratorModel: sonnet
# Command that starts Claude Code
claudeCommand: claude
# Arguments added to every agent session. The agents already run in bypassPermissions.
agentArgs: []
# Repositories in which the orchestrator can launch agents
repos: []
#  - name: atlas
#    path: /path/to/the/repository
#    setup: pnpm install --frozen-lockfile   # run in each new worktree
supervisorIntervalSeconds: 30
# Workflow of an agent launched without a precise one (bricks of ~/.atlas/bricks)
defaultSteps: [qualification, plan, implementation, tests, review, correction, ship]
# Name of a new branch
branchPattern: "{type}/{ticket}/{name}"
"#;

/// Written to `~/.atlas/config.yml` on first run, commented in the language in service.
fn default_config_yaml() -> &'static str {
    match language::current() {
        Language::Fr => DEFAULT_CONFIG_YAML_FR,
        Language::En => DEFAULT_CONFIG_YAML_EN,
    }
}

/// A problem in a file the master edits, worded so a typo is fixed without reading a stack trace.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ConfigError(pub String);

fn default_port() -> u16 {
    atlas_link::DEFAULT_PORT
}
fn default_orchestrator_model() -> String {
    "sonnet".to_owned()
}
fn default_claude_command() -> String {
    "claude".to_owned()
}
fn default_supervisor_interval() -> u64 {
    30
}
fn default_max_context_tokens() -> u64 {
    60_000
}
fn default_approval_timeout() -> u64 {
    180
}
fn default_orchestrator_timeout() -> u64 {
    600
}
fn default_steps() -> Vec<String> {
    [
        "qualification",
        "plan",
        "implementation",
        "tests",
        "review",
        "correction",
        "ship",
    ]
    .map(str::to_owned)
    .to_vec()
}
fn default_branch_pattern() -> String {
    "{type}/{ticket}/{name}".to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtlasConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_orchestrator_model")]
    pub orchestrator_model: String,
    #[serde(default = "default_claude_command")]
    pub claude_command: String,
    /// Extra arguments for every agent session, e.g. a permission mode.
    #[serde(default)]
    pub agent_args: Vec<String>,
    /// Projects imported into the registry at startup; the orchestrator can add more (add_repo).
    #[serde(default)]
    pub repos: Vec<RepoInput>,
    #[serde(default = "default_supervisor_interval")]
    pub supervisor_interval_seconds: u64,
    /// Context size past which the orchestrator's chat moves to a fresh session.
    #[serde(default = "default_max_context_tokens")]
    pub orchestrator_max_context_tokens: u64,
    /// Seconds an agent waits for the orchestrator to validate a step before it is approved as it stands.
    #[serde(default = "default_approval_timeout")]
    pub approval_timeout_seconds: u64,
    /// Seconds a run of the orchestrator may take before it is given up: a `claude -p` that hangs
    /// would otherwise hold every later message and anomaly back.
    #[serde(default = "default_orchestrator_timeout")]
    pub orchestrator_timeout_seconds: u64,
    /// Workflow of an agent launched without one, as brick ids.
    #[serde(default = "default_steps")]
    pub default_steps: Vec<String>,
    /// Name of a new branch: {type}, {ticket} and {name} are filled at launch.
    #[serde(default = "default_branch_pattern")]
    pub branch_pattern: String,
}

impl Default for AtlasConfig {
    fn default() -> Self {
        serde_yaml_ng::from_str("{}").expect("an empty configuration takes every default")
    }
}

impl AtlasConfig {
    fn validated(mut self) -> Result<Self, Vec<Issue>> {
        let mut issues = Vec::new();
        let mut require = |is_valid: bool, path: &str, message: &str| {
            if !is_valid {
                issues.push(Issue::new(path, message));
            }
        };
        require(
            self.port > 0,
            "port",
            &tr!("entier positif attendu", "positive integer expected"),
        );
        require(
            !self.orchestrator_model.is_empty(),
            "orchestratorModel",
            &tr!("texte vide", "empty text"),
        );
        require(
            !self.claude_command.is_empty(),
            "claudeCommand",
            &tr!("texte vide", "empty text"),
        );
        require(
            self.supervisor_interval_seconds > 0,
            "supervisorIntervalSeconds",
            &tr!("entier positif attendu", "positive integer expected"),
        );
        require(
            self.orchestrator_max_context_tokens > 0,
            "orchestratorMaxContextTokens",
            &tr!("entier positif attendu", "positive integer expected"),
        );
        require(
            self.approval_timeout_seconds > 0,
            "approvalTimeoutSeconds",
            &tr!("entier positif attendu", "positive integer expected"),
        );
        require(
            self.orchestrator_timeout_seconds > 0,
            "orchestratorTimeoutSeconds",
            &tr!("entier positif attendu", "positive integer expected"),
        );
        let has_steps = !self.default_steps.is_empty() && self.default_steps.iter().all(|step| !step.is_empty());
        require(
            has_steps,
            "defaultSteps",
            &tr!("au moins une brique", "at least one brick"),
        );
        require(
            !self.branch_pattern.is_empty(),
            "branchPattern",
            &tr!("texte vide", "empty text"),
        );
        let mut repos = Vec::new();
        for (index, repo) in std::mem::take(&mut self.repos).into_iter().enumerate() {
            match repo.validated() {
                Ok(repo) => repos.push(repo),
                Err(repo_issues) => issues.extend(
                    repo_issues
                        .into_iter()
                        .map(|issue| Issue::new(&format!("repos.{index}.{}", issue.path), issue.message)),
                ),
            }
        }
        self.repos = repos;
        if issues.is_empty() { Ok(self) } else { Err(issues) }
    }
}

/// Parses a YAML file the master edits, then checks it with `validate`. Every problem becomes a
/// `ConfigError` naming the file and each invalid key.
pub fn parse_yaml_file<T: DeserializeOwned>(
    source: &str,
    file: &str,
    validate: impl FnOnce(T) -> Result<T, Vec<Issue>>,
) -> Result<T, ConfigError> {
    // An empty file is an empty mapping: every key takes its default.
    let source = if source.trim().is_empty() { "{}" } else { source };
    let parsed: T = serde_yaml_ng::from_str(source)
        .map_err(|error| ConfigError(tr!("{file} est invalide : {error}", "{file} is invalid: {error}")))?;
    validate(parsed).map_err(|issues| {
        let lines: Vec<String> = issues
            .iter()
            .map(|issue| tr!("  - {} : {}", "  - {}: {}", issue.path, issue.message))
            .collect();
        ConfigError(tr!(
            "{file} est invalide :\n{}",
            "{file} is invalid:\n{}",
            lines.join("\n")
        ))
    })
}

pub fn parse_config(source: &str, file: &str) -> Result<AtlasConfig, ConfigError> {
    parse_yaml_file(source, file, AtlasConfig::validated)
}

/// Loads `config.yml` from `home`, writing the default on first run.
pub fn load_config(home: &Path) -> Result<AtlasConfig, ConfigError> {
    let unreadable =
        |path: &Path, error: std::io::Error| ConfigError(tr!("{} : {error}", "{}: {error}", path.display()));
    std::fs::create_dir_all(home).map_err(|error| unreadable(home, error))?;
    let path: PathBuf = atlas_link::config_path(home);
    if !path.exists() {
        std::fs::write(&path, default_config_yaml()).map_err(|error| unreadable(&path, error))?;
    }
    let source = std::fs::read_to_string(&path).map_err(|error| unreadable(&path, error))?;
    parse_config(&source, &path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_default_file_written_on_first_run() {
        let home = tempfile::tempdir().expect("home");
        let config = load_config(home.path()).expect("config");
        assert_eq!(config, AtlasConfig::default());
        assert_eq!((config.port, config.approval_timeout_seconds), (4317, 180));
        assert_eq!(
            std::fs::read_to_string(home.path().join("config.yml")).expect("file"),
            default_config_yaml()
        );
    }

    #[test]
    fn reads_projects_with_their_setup_and_ignores_unknown_keys() {
        let source = "port: 4400\nfuture: true\nrepos:\n  - name: atlas\n    path: /repo\n    ticketPrefixes: [ATLAS]\n    setup: pnpm install\n";
        let config = parse_config(source, "config.yml").expect("config");
        assert_eq!(config.port, 4400);
        assert_eq!(config.repos[0].setup, Some(Some("pnpm install".to_owned())));
    }

    #[test]
    fn names_the_file_and_each_invalid_key() {
        let error = parse_config(
            "defaultSteps: []\nrepos:\n  - name: 'a b'\n    path: /repo\n",
            "config.yml",
        )
        .expect_err("error");
        assert_eq!(
            error.0,
            "config.yml est invalide :\n  - defaultSteps : au moins une brique\n  - repos.0.name : nom : lettres, chiffres, point, tiret, souligné"
        );
    }

    #[test]
    fn reports_a_file_that_is_not_yaml_or_has_a_wrong_type() {
        assert!(
            parse_config("port: [", "config.yml")
                .expect_err("yaml")
                .0
                .starts_with("config.yml est invalide : ")
        );
        assert!(
            parse_config("port: many", "config.yml")
                .expect_err("type")
                .0
                .contains("port")
        );
    }
}
