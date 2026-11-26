// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The projects the orchestrator can launch agents in, and their memory: notes every later agent
//! of a project reads at the start of its session, plus notes that hold for every project.

use crate::language::tr;
use std::path::Path;

use crate::contract::{CreateAgentInput, RepoInput, RepoView, inline_issues, parse_repo_note, parse_repo_setup};
use crate::error::{Error, Result, refuse};
use crate::store::{RepoRecord, Store};

const GLOBAL_NOTES_KEY: &str = "notes.global";

/// Where a new agent works: the project's repository and the branch its work starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchRepo {
    pub path: String,
    pub base_branch: Option<String>,
}

/// The ticket prefix: `CAIRN-123` gives `CAIRN`.
pub fn ticket_prefix(ticket_ref: &str) -> String {
    let Some((prefix, _)) = ticket_ref.split_once('-') else {
        return String::new();
    };
    let mut chars = prefix.chars();
    let is_prefix = chars.next().is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_');
    if is_prefix {
        prefix.to_uppercase()
    } else {
        String::new()
    }
}

fn folder_name(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
}

/// A project reference reduced to its letters and digits: "Harbor Tools" and "harbor-tools" meet.
fn comparable(reference: &str) -> String {
    folder_name(reference.trim())
        .to_lowercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

fn parse_note(note: &str) -> Result<String> {
    parse_repo_note(note).map_err(|reason| Error::refusal(tr!("note refusée : {reason}", "note refused: {reason}")))
}

/// Valid notes, each kept once.
fn parse_notes(notes: &[String]) -> Result<Vec<String>> {
    let mut parsed: Vec<String> = Vec::new();
    for note in notes {
        let note = parse_repo_note(note)
            .map_err(|reason| Error::refusal(tr!("notes refusées : {reason}", "notes refused: {reason}")))?;
        if !parsed.contains(&note) {
            parsed.push(note);
        }
    }
    Ok(parsed)
}

#[derive(Clone)]
pub struct ProjectRegistry {
    store: Store,
}

impl ProjectRegistry {
    pub fn new(store: Store) -> Self {
        Self { store }
    }

    pub fn list(&self) -> Result<Vec<RepoView>> {
        self.store.list_repos()
    }

    pub fn find_by_path(&self, path: &str) -> Result<Option<RepoView>> {
        Ok(self.list()?.into_iter().find(|repo| repo.path == path))
    }

    /// The project an agent or the master means by `reference`: its name, its path, or the name of
    /// its folder, which is how a repository is called on the machine. Case, dashes and spaces do
    /// not count.
    pub fn find_by_reference(&self, reference: &str) -> Result<Option<RepoView>> {
        let wanted = comparable(reference);
        let repos = self.list()?;
        let exact = repos
            .iter()
            .position(|repo| repo.name == reference || repo.path == reference);
        let close = || {
            repos
                .iter()
                .position(|repo| comparable(&repo.name) == wanted || comparable(folder_name(&repo.path)) == wanted)
        };
        Ok(exact.or_else(close).map(|index| repos[index].clone()))
    }

    fn known_names(&self) -> Result<String> {
        let names: Vec<String> = self.list()?.into_iter().map(|repo| repo.name).collect();
        Ok(if names.is_empty() {
            tr!("aucun", "none")
        } else {
            names.join(", ")
        })
    }

    pub fn require(&self, name: &str) -> Result<RepoView> {
        match self.store.get_repo(name)? {
            Some(repo) => Ok(repo),
            None => refuse!(
                "projet inconnu : {name}. Projets connus : {}",
                "unknown project: {name}. Known projects: {}",
                self.known_names()?
            ),
        }
    }

    /// Registers a project the orchestrator can launch agents in; the path must be a git repository.
    pub async fn add(&self, input: RepoInput) -> Result<RepoView> {
        let input = input
            .validated()
            .map_err(|issues| Error::refusal(inline_issues(&issues, false)))?;
        let path = input.path.clone();
        let root = tokio::task::spawn_blocking(move || foundry_git::repository_root(&path))
            .await
            .map_err(|error| Error::internal(error.to_string()))?;
        let Some(root) = root else {
            refuse!("{} n'est pas un dépôt git", "{} is not a git repository", input.path);
        };
        let repo = self.store.save_repo(&RepoRecord {
            name: input.name,
            path: root,
            ticket_prefixes: input.ticket_prefixes,
            base_branch: input.base_branch,
        })?;
        for note in &input.notes {
            self.add_note(&repo.name, note)?;
        }
        if let Some(setup) = input.setup {
            self.set_setup(&repo.name, setup.as_deref())?;
        }
        self.require(&repo.name)
    }

    pub fn remove(&self, name: &str) -> Result<()> {
        if !self.store.remove_repo(name)? {
            refuse!("projet inconnu : {name}", "unknown project: {name}");
        }
        Ok(())
    }

    /// The project a launch targets: the repository it names by path (declared or not), else the
    /// project it names, else the one declaring the prefix of its ticket.
    pub fn resolve_for_launch(&self, input: &CreateAgentInput) -> Result<LaunchRepo> {
        if let Some(path) = &input.repo_path {
            let base_branch = self.find_by_path(path)?.and_then(|repo| repo.base_branch);
            return Ok(LaunchRepo {
                path: path.clone(),
                base_branch,
            });
        }
        if let Some(name) = &input.repo {
            let repo = self.require(name)?;
            return Ok(LaunchRepo {
                path: repo.path,
                base_branch: repo.base_branch,
            });
        }
        let prefix = ticket_prefix(&input.ticket_ref);
        let mut matches: Vec<RepoView> = self
            .list()?
            .into_iter()
            .filter(|repo| {
                repo.ticket_prefixes
                    .iter()
                    .any(|candidate| candidate.to_uppercase() == prefix)
            })
            .collect();
        match matches.len() {
            1 => {
                let repo = matches.remove(0);
                Ok(LaunchRepo {
                    path: repo.path,
                    base_branch: repo.base_branch,
                })
            }
            0 => refuse!(
                "aucun projet ne déclare le préfixe {}. Projets connus : {}. Précise le projet, ou ajoute-le avec add_repo.",
                "no project declares the prefix {}. Known projects: {}. Name the project, or add it with add_repo.",
                if prefix.is_empty() {
                    tr!("(vide)", "(empty)")
                } else {
                    prefix
                },
                self.known_names()?
            ),
            _ => {
                let names: Vec<String> = matches.into_iter().map(|repo| repo.name).collect();
                refuse!(
                    "plusieurs projets déclarent ce préfixe : {}. Précise le projet.",
                    "several projects declare this prefix: {}. Name the project.",
                    names.join(", ")
                )
            }
        }
    }

    /// Notes that hold for every project, present and future.
    pub fn list_global_notes(&self) -> Result<Vec<String>> {
        Ok(match self.store.get_value(GLOBAL_NOTES_KEY)? {
            Some(source) => serde_json::from_str(&source)?,
            None => Vec::new(),
        })
    }

    /// Rewrites the notes common to every project.
    pub fn set_global_notes(&self, notes: &[String]) -> Result<Vec<String>> {
        self.store
            .set_value(GLOBAL_NOTES_KEY, &serde_json::to_string(&parse_notes(notes)?)?)?;
        self.list_global_notes()
    }

    pub fn add_global_note(&self, note: &str) -> Result<Vec<String>> {
        let mut notes = self.list_global_notes()?;
        notes.push(parse_note(note)?);
        self.set_global_notes(&notes)
    }

    /// Adds one line to a project's memory; a line it already holds changes nothing.
    pub fn add_note(&self, name: &str, note: &str) -> Result<RepoView> {
        let repo = self.require(name)?;
        let parsed = parse_note(note)?;
        if repo.notes.contains(&parsed) {
            return Ok(repo);
        }
        let mut notes = repo.notes;
        notes.push(parsed);
        self.set_notes(name, &notes)
    }

    /// Sets the command Atlas runs in each new worktree of the project to install its dependencies; none removes it.
    pub fn set_setup(&self, name: &str, setup: Option<&str>) -> Result<RepoView> {
        self.require(name)?;
        let parsed = setup
            .map(|command| {
                parse_repo_setup(command)
                    .map_err(|reason| Error::refusal(tr!("commande refusée : {reason}", "command refused: {reason}")))
            })
            .transpose()?;
        self.store.set_repo_setup(name, parsed.as_deref())?;
        self.require(name)
    }

    /// Rewrites a project's memory, to correct or drop notes.
    pub fn set_notes(&self, name: &str, notes: &[String]) -> Result<RepoView> {
        self.require(name)?;
        self.store.set_repo_notes(name, &parse_notes(notes)?)?;
        self.require(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::tests::TestClock;
    use crate::testing::refusal;

    fn registry() -> ProjectRegistry {
        let store = Store::in_memory(TestClock::new().clock()).expect("store");
        for (name, path, prefix) in [
            ("shop", "/repos/shop-api", "SHOP"),
            ("dock-cli", "/repos/harbor-tools", "DOCK"),
        ] {
            let repo = RepoRecord {
                name: name.into(),
                path: path.into(),
                ticket_prefixes: vec![prefix.into()],
                base_branch: Some("main".into()),
            };
            store.save_repo(&repo).expect("repo");
        }
        ProjectRegistry::new(store)
    }

    fn launch(ticket_ref: &str) -> CreateAgentInput {
        CreateAgentInput {
            ticket_ref: ticket_ref.to_owned(),
            ..CreateAgentInput::default()
        }
    }

    #[test]
    fn reads_the_prefix_of_a_ticket() {
        assert_eq!(ticket_prefix("cairn-123"), "CAIRN");
        assert_eq!(ticket_prefix("SHOP_2-9-x"), "SHOP_2");
        assert_eq!(ticket_prefix("review de la MR"), "");
        assert_eq!(ticket_prefix("9X-1"), "");
    }

    #[test]
    fn finds_a_project_by_name_path_or_folder_whatever_the_spelling() {
        let registry = registry();
        let name = |reference: &str| {
            registry
                .find_by_reference(reference)
                .expect("find")
                .map(|repo| repo.name)
        };
        assert_eq!(name("shop").as_deref(), Some("shop"));
        assert_eq!(name("/repos/shop-api").as_deref(), Some("shop"));
        assert_eq!(name("Harbor Tools").as_deref(), Some("dock-cli"));
        assert_eq!(name("DOCK_CLI").as_deref(), Some("dock-cli"));
        assert_eq!(name("elsewhere"), None);
    }

    #[test]
    fn resolves_a_launch_by_path_then_name_then_ticket_prefix() {
        let registry = registry();
        let by_path = CreateAgentInput {
            repo_path: Some("/anywhere".into()),
            ..launch("X-1")
        };
        assert_eq!(
            registry.resolve_for_launch(&by_path).expect("path"),
            LaunchRepo {
                path: "/anywhere".into(),
                base_branch: None
            }
        );
        let by_name = CreateAgentInput {
            repo: Some("dock-cli".into()),
            ..launch("SHOP-1")
        };
        assert_eq!(
            registry.resolve_for_launch(&by_name).expect("name").path,
            "/repos/harbor-tools"
        );
        assert_eq!(
            registry.resolve_for_launch(&launch("shop-12")).expect("prefix").path,
            "/repos/shop-api"
        );
    }

    #[test]
    fn refuses_a_launch_no_project_or_several_projects_claim() {
        let registry = registry();
        assert_eq!(
            refusal(registry.resolve_for_launch(&launch("OTHER-1"))),
            "aucun projet ne déclare le préfixe OTHER. Projets connus : dock-cli, shop. Précise le projet, ou ajoute-le avec add_repo."
        );
        assert!(refusal(registry.resolve_for_launch(&launch("libre"))).contains("le préfixe (vide)"));
        let twin = RepoRecord {
            name: "shop-web".into(),
            path: "/repos/shop-web".into(),
            ticket_prefixes: vec!["SHOP".into()],
            base_branch: None,
        };
        registry.store.save_repo(&twin).expect("twin");
        assert_eq!(
            refusal(registry.resolve_for_launch(&launch("SHOP-1"))),
            "plusieurs projets déclarent ce préfixe : shop, shop-web. Précise le projet."
        );
    }

    #[test]
    fn keeps_each_note_once_and_refuses_a_note_on_several_lines() {
        let registry = registry();
        registry.add_note("shop", " host is api.test ").expect("note");
        assert_eq!(
            registry.add_note("shop", "host is api.test").expect("again").notes,
            ["host is api.test"]
        );
        assert_eq!(
            refusal(registry.add_note("shop", "two\nlines")),
            "note refusée : une seule ligne"
        );
        assert!(
            refusal(registry.add_note("ghost", "x"))
                .starts_with("projet inconnu : ghost. Projets connus : dock-cli, shop")
        );
        assert_eq!(
            registry
                .set_notes("shop", &["a".into(), "a".into(), "b".into()])
                .expect("set")
                .notes,
            ["a", "b"]
        );
    }

    #[test]
    fn keeps_notes_for_every_project_apart() {
        let registry = registry();
        assert!(registry.list_global_notes().expect("empty").is_empty());
        registry.add_global_note("commits en anglais").expect("add");
        assert_eq!(
            registry.add_global_note("commits en anglais").expect("again"),
            ["commits en anglais"]
        );
        assert!(registry.set_global_notes(&[]).expect("clear").is_empty());
    }

    #[test]
    fn sets_and_removes_the_setup_command() {
        let registry = registry();
        assert_eq!(
            registry
                .set_setup("shop", Some(" pnpm install "))
                .expect("set")
                .setup
                .as_deref(),
            Some("pnpm install")
        );
        assert_eq!(
            refusal(registry.set_setup("shop", Some("a\nb"))),
            "commande refusée : une seule ligne"
        );
        assert_eq!(registry.set_setup("shop", None).expect("remove").setup, None);
    }

    #[tokio::test]
    async fn adds_a_project_at_the_root_of_its_repository_and_refuses_a_plain_folder() {
        let registry = registry();
        let dir = tempfile::tempdir().expect("dir");
        let root = dir.path().canonicalize().expect("canonical");
        let input = |path: &Path| RepoInput {
            name: "new".into(),
            path: path.to_string_lossy().into_owned(),
            notes: vec!["note".into()],
            ..RepoInput::default()
        };
        assert!(refusal(registry.add(input(&root)).await).ends_with("n'est pas un dépôt git"));
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["init", "--quiet"])
                .status()
                .expect("git")
                .success()
        );
        std::fs::create_dir(root.join("sub")).expect("sub");
        let added = registry.add(input(&root.join("sub"))).await.expect("added");
        assert_eq!(
            (added.path.as_str(), added.notes.as_slice()),
            (root.to_str().expect("utf8"), ["note".to_owned()].as_slice())
        );
        assert_eq!(
            refusal(
                registry
                    .add(RepoInput {
                        name: "a b".into(),
                        ..input(&root)
                    })
                    .await
            ),
            "nom : lettres, chiffres, point, tiret, souligné"
        );
    }
}
