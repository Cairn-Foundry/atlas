// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Git working folders: the worktree a piece of work happens in, found or created.
//!
//! Everything goes through the `git` command of the machine, so remotes, credentials and hooks
//! behave as they do in the user's own terminal.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Runs `git -C <repo_path> <args>` and returns its trimmed standard output.
fn git(repo_path: &str, args: &[&str]) -> Result<String, WorkspaceError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(args)
        // Nobody sits at this terminal: a remote asking for a password fails instead of waiting for ever.
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|error| WorkspaceError::Git(format!("git introuvable : {error}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(WorkspaceError::Git(format!("git {} : {stderr}", args.join(" "))));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn ref_exists(repo_path: &str, reference: &str) -> bool {
    git(repo_path, &["rev-parse", "--verify", "--quiet", reference]).is_ok()
}

/// Why a working folder could not be found or created.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WorkspaceError {
    /// The main working tree has a detached head.
    #[error("le dossier principal du projet n'est sur aucune branche")]
    MainWorktreeDetached,
    /// The main working tree is on `actual`, and nothing here ever switches its branch.
    #[error("le dossier principal est sur {actual}, pas sur {wanted}")]
    MainWorktreeOnOtherBranch { actual: String, wanted: String },
    /// The path asked for is not a worktree of the repository.
    #[error("{path} n'est pas un worktree de ce projet")]
    NotAWorktree { path: String },
    /// The worktree asked for has a detached head.
    #[error("{path} n'est sur aucune branche")]
    WorktreeDetached { path: String },
    #[error("{path} est sur {actual}, pas sur {wanted}")]
    WorktreeOnOtherBranch {
        path: String,
        actual: String,
        wanted: String,
    },
    /// The worktree asked for is the main working tree, which is only used on an explicit request.
    #[error("ce chemin est le dossier principal du projet")]
    WorktreeIsMain,
    /// The branch is checked out in the main working tree, which is only used on an explicit request.
    #[error("{branch} est ouverte dans le dossier principal du projet")]
    BranchOpenInMain { branch: String },
    /// The branch exists neither locally nor on `origin`.
    #[error("la branche {branch} n'existe ni dans le dépôt ni sur origin")]
    BranchNotFound { branch: String },
    /// The name could be read by git as an option or as something other than one branch.
    #[error("nom de branche invalide : {branch}")]
    InvalidBranchName { branch: String },
    /// A git command failed; the text holds the command and what it printed.
    #[error("{0}")]
    Git(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: String,
    /// Branch checked out there, none when the head is detached.
    pub branch: Option<String>,
}

/// Parses `git worktree list --porcelain`; the first entry is the main working tree.
pub fn parse_worktree_list(output: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut current: Option<WorktreeEntry> = None;
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            entries.extend(current.take());
            current = Some(WorktreeEntry {
                path: path.to_owned(),
                branch: None,
            });
        } else if let (Some(entry), Some(branch)) = (current.as_mut(), line.strip_prefix("branch ")) {
            entry.branch = Some(branch.strip_prefix("refs/heads/").unwrap_or(branch).to_owned());
        }
    }
    entries.extend(current);
    entries
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceRequest {
    pub repo_path: String,
    /// Branch new work starts from, none for the branch checked out in the repository.
    pub base_branch: Option<String>,
    /// Where a worktree is created when one is needed.
    pub new_worktree_path: String,
    /// Name of the branch to create when no existing branch or worktree is asked for.
    pub new_branch: String,
    /// Existing branch to work on, local or only on the remote.
    pub branch: Option<String>,
    /// Existing worktree to work in.
    pub worktree_path: Option<String>,
    /// Work in the main working tree of the repository.
    pub in_place: bool,
    /// New worktree without a branch, for work that reads and never commits.
    pub is_detached: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    /// Main working tree of the repository, even when `repo_path` is itself a linked worktree.
    pub repo_root: String,
    /// Folder the work happens in.
    pub path: String,
    /// Branch of the work, empty for a folder without one.
    pub branch: String,
    /// Branch the work is compared to in review.
    pub base_branch: String,
    /// True when the folder was created here: only then may it ever be removed.
    pub is_created: bool,
}

/// Whether `name` can be handed to git as a branch: never an option (a leading dash), a revision
/// expression or a refspec. Branch names come from tickets, agents and merge requests.
fn is_branch_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['-', '/', '.'])
        && !name.ends_with(['/', '.'])
        && !name.contains("..")
        && !name.contains("//")
        && !name.contains("@{")
        && !name
            .chars()
            .any(|character| character.is_whitespace() || character.is_control() || "~^:?*[\\".contains(character))
}

/// The path as git records it, links resolved, so a folder reached through a link is still found.
fn absolute(path: &str) -> PathBuf {
    std::fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| PathBuf::from(path))
}

/// Finds or creates the folder a piece of work happens in:
/// - `in_place`: the main working tree, on the branch checked out there;
/// - `worktree_path`: that existing worktree;
/// - `branch`: the worktree where it is already checked out, else a new worktree on it, taking the
///   branch from the remote when it only exists there;
/// - `is_detached`: a new worktree with no branch, at the base;
/// - nothing: the worktree where `new_branch` is already checked out, else a new worktree on it,
///   started from `origin/<base_branch>` freshly fetched, from the local base when there is no
///   remote, or from the branch checked out.
pub fn prepare_workspace(request: &WorkspaceRequest) -> Result<Workspace, WorkspaceError> {
    let repo_path = request.repo_path.as_str();
    let named = [request.branch.as_deref(), request.base_branch.as_deref()];
    let created =
        (request.branch.is_none() && !request.is_detached && !request.in_place).then_some(&*request.new_branch);
    if let Some(invalid) = named
        .into_iter()
        .flatten()
        .chain(created)
        .find(|name| !is_branch_name(name))
    {
        return Err(WorkspaceError::InvalidBranchName {
            branch: invalid.to_owned(),
        });
    }
    let common_dir = git(repo_path, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let repo_root = Path::new(&common_dir)
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or(common_dir);
    // A worktree whose folder was deleted by hand stays on git's list with its branch: forgotten
    // first, so the work never lands in a folder that is gone.
    let _ = git(repo_path, &["worktree", "prune"]);
    let worktrees = parse_worktree_list(&git(repo_path, &["worktree", "list", "--porcelain"])?);
    let main = worktrees.first();
    let base_branch = match &request.base_branch {
        Some(base) => base.clone(),
        None => git(&repo_root, &["rev-parse", "--abbrev-ref", "HEAD"])?,
    };
    let existing = |entry: &WorktreeEntry, branch: &str| Workspace {
        repo_root: repo_root.clone(),
        path: entry.path.clone(),
        branch: branch.to_owned(),
        base_branch: base_branch.clone(),
        is_created: false,
    };
    let created = |branch: &str| Workspace {
        repo_root: repo_root.clone(),
        path: request.new_worktree_path.clone(),
        branch: branch.to_owned(),
        base_branch: base_branch.clone(),
        is_created: true,
    };

    if request.in_place {
        let Some((main, main_branch)) = main.and_then(|entry| entry.branch.as_ref().map(|branch| (entry, branch)))
        else {
            return Err(WorkspaceError::MainWorktreeDetached);
        };
        if let Some(wanted) = request.branch.as_ref().filter(|wanted| *wanted != main_branch) {
            return Err(WorkspaceError::MainWorktreeOnOtherBranch {
                actual: main_branch.clone(),
                wanted: wanted.clone(),
            });
        }
        return Ok(existing(main, main_branch));
    }

    if let Some(worktree_path) = &request.worktree_path {
        let wanted = absolute(worktree_path);
        let Some(entry) = worktrees.iter().find(|candidate| absolute(&candidate.path) == wanted) else {
            return Err(WorkspaceError::NotAWorktree {
                path: worktree_path.clone(),
            });
        };
        let Some(entry_branch) = &entry.branch else {
            return Err(WorkspaceError::WorktreeDetached {
                path: worktree_path.clone(),
            });
        };
        if let Some(wanted_branch) = request.branch.as_ref().filter(|wanted| *wanted != entry_branch) {
            return Err(WorkspaceError::WorktreeOnOtherBranch {
                path: worktree_path.clone(),
                actual: entry_branch.clone(),
                wanted: wanted_branch.clone(),
            });
        }
        if main.is_some_and(|main| std::ptr::eq(main, entry)) {
            return Err(WorkspaceError::WorktreeIsMain);
        }
        return Ok(existing(entry, entry_branch));
    }

    // A branch git already has checked out cannot get a second worktree: the work happens in that one.
    let open_worktree = |branch: &str| -> Result<Option<Workspace>, WorkspaceError> {
        let Some(entry) = worktrees
            .iter()
            .find(|candidate| candidate.branch.as_deref() == Some(branch))
        else {
            return Ok(None);
        };
        if main.is_some_and(|main| std::ptr::eq(main, entry)) {
            return Err(WorkspaceError::BranchOpenInMain {
                branch: branch.to_owned(),
            });
        }
        Ok(Some(existing(entry, branch)))
    };
    let new_path = request.new_worktree_path.as_str();

    if let Some(branch) = &request.branch {
        if let Some(open) = open_worktree(branch)? {
            return Ok(open);
        }
        if ref_exists(repo_path, &format!("refs/heads/{branch}")) {
            git(repo_path, &["worktree", "add", new_path, branch])?;
        } else {
            let _ = git(repo_path, &["fetch", "--quiet", "origin", branch]);
            if !ref_exists(repo_path, &format!("refs/remotes/origin/{branch}")) {
                return Err(WorkspaceError::BranchNotFound { branch: branch.clone() });
            }
            let remote = format!("origin/{branch}");
            git(
                repo_path,
                &["worktree", "add", "--track", "-b", branch, new_path, &remote],
            )?;
        }
        return Ok(created(branch));
    }

    let start_point = request.base_branch.as_ref().map(|base| {
        // Offline, what origin held at the last fetch is still closer to it than a local branch
        // nobody pulls.
        if git(repo_path, &["fetch", "--quiet", "origin", base]).is_ok()
            || ref_exists(repo_path, &format!("refs/remotes/origin/{base}"))
        {
            format!("origin/{base}")
        } else {
            base.clone()
        }
    });
    if request.is_detached {
        let mut args = vec!["worktree", "add", "--detach", new_path];
        args.extend(start_point.as_deref());
        git(repo_path, &args)?;
        return Ok(created(""));
    }
    if let Some(open) = open_worktree(&request.new_branch)? {
        return Ok(open);
    }
    if ref_exists(repo_path, &format!("refs/heads/{}", request.new_branch)) {
        git(repo_path, &["worktree", "add", new_path, &request.new_branch])?;
    } else {
        let mut args = vec!["worktree", "add", "-b", &request.new_branch, new_path];
        args.extend(start_point.as_deref());
        git(repo_path, &args)?;
    }
    Ok(created(&request.new_branch))
}

/// Removes a worktree created by `prepare_workspace` and no longer used, with its branch. Best
/// effort and never forced: a folder with uncommitted files, or a branch holding commits found
/// nowhere else, stays, since something in it may still matter to its owner.
pub fn discard_workspace(repo_path: &str, path: &str, branch: &str) {
    if git(repo_path, &["worktree", "remove", path]).is_ok() && !branch.is_empty() {
        let _ = git(repo_path, &["branch", "-d", branch]);
    }
}

/// Whether everything in the folder is committed; true when git cannot tell, so a broken git never traps its caller.
pub fn is_tree_clean(path: &str) -> bool {
    git(path, &["status", "--porcelain"]).map_or(true, |status| status.is_empty())
}

/// Root of the working tree at `path`, none when it is not a git repository.
pub fn repository_root(path: &str) -> Option<String> {
    git(path, &["rev-parse", "--show-toplevel"]).ok()
}

/// Commit the folder is at, none when git cannot tell.
pub fn head_commit(path: &str) -> Option<String> {
    git(path, &["rev-parse", "HEAD"]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git").arg("-C").arg(dir).args(args).output().expect("git");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// A repository on `main` with one commit, at the path git reports for it: links resolved, and
    /// on Windows the form git writes rather than the verbatim one `canonicalize` gives and git refuses.
    fn repository() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("repo");
        std::fs::create_dir(&root).expect("repo dir");
        run(&root, &["init", "--quiet", "--initial-branch=main"]);
        run(&root, &["config", "user.email", "atlas@example.test"]);
        run(&root, &["config", "user.name", "Atlas"]);
        run(&root, &["commit", "--quiet", "--allow-empty", "-m", "init"]);
        let root = PathBuf::from(run(&root, &["rev-parse", "--show-toplevel"]));
        (dir, root)
    }

    fn request(root: &Path, worktree: &str) -> WorkspaceRequest {
        WorkspaceRequest {
            repo_path: root.to_string_lossy().into_owned(),
            new_worktree_path: root
                .parent()
                .expect("parent")
                .join(worktree)
                .to_string_lossy()
                .into_owned(),
            new_branch: "feat/T-1/work".into(),
            ..WorkspaceRequest::default()
        }
    }

    #[test]
    fn parses_the_porcelain_list_with_a_detached_worktree() {
        let output = "worktree /repo\nHEAD abc\nbranch refs/heads/main\n\nworktree /wt\nHEAD def\ndetached\n";
        assert_eq!(
            parse_worktree_list(output),
            vec![
                WorktreeEntry {
                    path: "/repo".into(),
                    branch: Some("main".into())
                },
                WorktreeEntry {
                    path: "/wt".into(),
                    branch: None
                },
            ]
        );
    }

    #[test]
    fn creates_a_worktree_on_a_new_branch_from_the_checked_out_one() {
        let (_dir, root) = repository();
        let workspace = prepare_workspace(&request(&root, "wt")).expect("workspace");
        assert_eq!(workspace.branch, "feat/T-1/work");
        assert_eq!(workspace.base_branch, "main");
        assert_eq!(workspace.repo_root, root.to_string_lossy());
        assert!(workspace.is_created);
        assert_eq!(
            run(Path::new(&workspace.path), &["rev-parse", "--abbrev-ref", "HEAD"]),
            "feat/T-1/work"
        );
    }

    #[test]
    fn reuses_the_worktree_where_the_branch_is_already_open() {
        let (_dir, root) = repository();
        let first = prepare_workspace(&request(&root, "wt")).expect("first");
        let second = prepare_workspace(&request(&root, "other")).expect("second");
        assert_eq!(second.path, first.path);
        assert!(!second.is_created);
    }

    #[test]
    fn creates_a_new_worktree_when_the_one_holding_the_branch_was_deleted_by_hand() {
        let (_dir, root) = repository();
        let first = prepare_workspace(&request(&root, "wt")).expect("first");
        std::fs::remove_dir_all(&first.path).expect("deleted");
        let second = prepare_workspace(&request(&root, "other")).expect("second");
        assert!(second.is_created && Path::new(&second.path).is_dir());
        assert_eq!(second.branch, first.branch);
    }

    #[test]
    fn creates_a_detached_worktree_for_work_that_never_commits() {
        let (_dir, root) = repository();
        let workspace = prepare_workspace(&WorkspaceRequest {
            is_detached: true,
            ..request(&root, "wt")
        })
        .expect("workspace");
        assert_eq!(workspace.branch, "");
        assert_eq!(
            run(Path::new(&workspace.path), &["rev-parse", "--abbrev-ref", "HEAD"]),
            "HEAD"
        );
    }

    #[test]
    fn opens_an_existing_local_branch_in_a_new_worktree() {
        let (_dir, root) = repository();
        run(&root, &["branch", "fix/existing"]);
        let workspace = prepare_workspace(&WorkspaceRequest {
            branch: Some("fix/existing".into()),
            ..request(&root, "wt")
        })
        .expect("workspace");
        assert_eq!(workspace.branch, "fix/existing");
        assert!(workspace.is_created);
    }

    /// The repository of `repository()` with an `origin` that holds one more commit on `main`
    /// and a branch, `fix/remote-only`, the local repository has never seen.
    fn repository_with_origin() -> (tempfile::TempDir, PathBuf, String) {
        let (dir, root) = repository();
        let base = root.parent().expect("parent").to_path_buf();
        let origin = base.join("origin.git");
        run(
            &base,
            &[
                "clone",
                "--quiet",
                "--bare",
                root.to_str().expect("utf8"),
                origin.to_str().expect("utf8"),
            ],
        );
        run(&root, &["remote", "add", "origin", origin.to_str().expect("utf8")]);
        let other = base.join("other");
        run(
            &base,
            &[
                "clone",
                "--quiet",
                origin.to_str().expect("utf8"),
                other.to_str().expect("utf8"),
            ],
        );
        run(&other, &["config", "user.email", "atlas@example.test"]);
        run(&other, &["config", "user.name", "Atlas"]);
        run(&other, &["commit", "--quiet", "--allow-empty", "-m", "ahead"]);
        run(&other, &["push", "--quiet", "origin", "main"]);
        let ahead = run(&other, &["rev-parse", "HEAD"]);
        run(
            &other,
            &["push", "--quiet", "origin", "main:refs/heads/fix/remote-only"],
        );
        (dir, root, ahead)
    }

    #[test]
    fn starts_new_work_from_the_base_freshly_fetched_from_origin() {
        let (_dir, root, ahead) = repository_with_origin();
        let workspace = prepare_workspace(&WorkspaceRequest {
            base_branch: Some("main".into()),
            ..request(&root, "wt")
        })
        .expect("workspace");
        // The local main is one commit behind: the work starts from what origin holds now.
        assert_eq!(head_commit(&workspace.path), Some(ahead));
        assert_ne!(head_commit(root.to_str().expect("utf8")), head_commit(&workspace.path));
    }

    #[test]
    fn starts_from_the_local_base_when_there_is_no_remote() {
        let (_dir, root) = repository();
        let workspace = prepare_workspace(&WorkspaceRequest {
            base_branch: Some("main".into()),
            ..request(&root, "wt")
        })
        .expect("workspace");
        assert_eq!(head_commit(&workspace.path), head_commit(root.to_str().expect("utf8")));
    }

    #[test]
    fn takes_from_origin_a_branch_that_only_exists_there() {
        let (_dir, root, ahead) = repository_with_origin();
        let request = WorkspaceRequest {
            branch: Some("fix/remote-only".into()),
            ..request(&root, "wt")
        };
        let workspace = prepare_workspace(&request).expect("workspace");
        assert_eq!(
            (workspace.branch.as_str(), workspace.is_created),
            ("fix/remote-only", true)
        );
        assert_eq!(head_commit(&workspace.path), Some(ahead));
        assert_eq!(
            run(
                Path::new(&workspace.path),
                &["rev-parse", "--abbrev-ref", "@{upstream}"]
            ),
            "origin/fix/remote-only"
        );
    }

    #[test]
    fn refuses_a_name_git_would_read_as_an_option_before_running_anything() {
        let (dir, root) = repository();
        let marker = dir.path().join("ran");
        let option = format!("--upload-pack=touch {};true", marker.display());
        run(&root, &["remote", "add", "origin", root.to_str().expect("utf8")]);
        for request in [
            WorkspaceRequest {
                branch: Some(option.clone()),
                ..request(&root, "wt")
            },
            WorkspaceRequest {
                base_branch: Some(option.clone()),
                ..request(&root, "wt")
            },
            WorkspaceRequest {
                new_branch: option.clone(),
                ..request(&root, "wt")
            },
        ] {
            let error = prepare_workspace(&request).expect_err("refusal");
            assert_eq!(error, WorkspaceError::InvalidBranchName { branch: option.clone() });
        }
        assert!(!marker.exists());
        for name in ["feat/SHOP-12/renameAccount", "release-1.2", "user@host"] {
            assert!(is_branch_name(name), "{name}");
        }
        for name in ["", "-x", "a..b", "a b", "a:b", "main@{1}", "a/", ".hidden", "a^"] {
            assert!(!is_branch_name(name), "{name}");
        }
    }

    #[test]
    fn refuses_a_branch_found_nowhere() {
        let (_dir, root) = repository();
        let error = prepare_workspace(&WorkspaceRequest {
            branch: Some("ghost".into()),
            ..request(&root, "wt")
        })
        .expect_err("refusal");
        assert_eq!(error, WorkspaceError::BranchNotFound { branch: "ghost".into() });
    }

    #[test]
    fn works_in_place_only_on_the_branch_checked_out_there() {
        let (_dir, root) = repository();
        let in_place = WorkspaceRequest {
            in_place: true,
            ..request(&root, "wt")
        };
        let workspace = prepare_workspace(&in_place).expect("workspace");
        assert_eq!(
            (workspace.path.as_str(), workspace.branch.as_str()),
            (root.to_str().expect("utf8"), "main")
        );
        assert!(!workspace.is_created);
        let error = prepare_workspace(&WorkspaceRequest {
            branch: Some("other".into()),
            ..in_place
        })
        .expect_err("refusal");
        assert_eq!(
            error,
            WorkspaceError::MainWorktreeOnOtherBranch {
                actual: "main".into(),
                wanted: "other".into()
            }
        );
    }

    #[test]
    fn refuses_the_main_folder_unless_asked_in_place() {
        let (_dir, root) = repository();
        let by_branch = WorkspaceRequest {
            branch: Some("main".into()),
            ..request(&root, "wt")
        };
        assert_eq!(
            prepare_workspace(&by_branch),
            Err(WorkspaceError::BranchOpenInMain { branch: "main".into() })
        );
        let by_path = WorkspaceRequest {
            worktree_path: Some(root.to_string_lossy().into_owned()),
            ..request(&root, "wt")
        };
        assert_eq!(prepare_workspace(&by_path), Err(WorkspaceError::WorktreeIsMain));
    }

    #[test]
    fn works_in_a_named_worktree_and_refuses_a_foreign_path() {
        let (_dir, root) = repository();
        let first = prepare_workspace(&request(&root, "wt")).expect("first");
        let named = WorkspaceRequest {
            worktree_path: Some(first.path.clone()),
            ..request(&root, "other")
        };
        assert_eq!(prepare_workspace(&named).expect("named").path, first.path);
        let foreign = WorkspaceRequest {
            worktree_path: Some("/nowhere".into()),
            ..request(&root, "other")
        };
        assert_eq!(
            prepare_workspace(&foreign),
            Err(WorkspaceError::NotAWorktree {
                path: "/nowhere".into()
            })
        );
    }

    #[test]
    fn discards_a_clean_worktree_with_its_branch_and_keeps_a_dirty_one() {
        let (_dir, root) = repository();
        let repo = root.to_string_lossy().into_owned();
        let clean = prepare_workspace(&request(&root, "clean")).expect("clean");
        discard_workspace(&repo, &clean.path, &clean.branch);
        assert!(!Path::new(&clean.path).exists());
        assert!(!ref_exists(&repo, "refs/heads/feat/T-1/work"));

        let dirty = prepare_workspace(&request(&root, "dirty")).expect("dirty");
        std::fs::write(Path::new(&dirty.path).join("draft.txt"), "work").expect("write");
        assert!(!is_tree_clean(&dirty.path));
        discard_workspace(&repo, &dirty.path, &dirty.branch);
        assert!(Path::new(&dirty.path).exists());
    }

    #[test]
    fn reads_the_root_and_head_and_answers_none_outside_a_repository() {
        let (_dir, root) = repository();
        assert_eq!(
            repository_root(root.to_str().expect("utf8")),
            Some(root.to_string_lossy().into_owned())
        );
        assert_eq!(
            head_commit(root.to_str().expect("utf8")),
            Some(run(&root, &["rev-parse", "HEAD"]))
        );
        let outside = root.parent().expect("parent");
        assert_eq!(repository_root(outside.to_str().expect("utf8")), None);
        assert!(is_tree_clean(outside.to_str().expect("utf8")));
    }
}
