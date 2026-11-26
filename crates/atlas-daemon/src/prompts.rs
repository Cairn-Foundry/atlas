// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What Atlas tells an agent session: its first prompt, the context its hooks inject and the brief
//! of each step. Pure text built from the agent's record, so it can be read and tested on its own.

use crate::brick::{Brick, NOTE_SECTION};
use crate::contract::{RepoView, StepState};
use crate::language;
use crate::store::{AgentRecord, ExtraWorkspace, StepRecord};
use crate::text::to_command_line_text;
use crate::workflow::{describe_path, looping_review, render_command};

pub const NO_BRANCH_NOTICE: &str = "You have no branch yet: this work is about code that already exists elsewhere. Before reading or changing the code, find the branch to take over (for a merge request: its source branch, and its target branch as the base) then call work_on_branch. Atlas settles you there.";
pub const NEW_STEP_PROMPT: &str =
    "New Atlas step: call next_step of the atlas MCP server for its instructions, then do it.";
pub const RESUME_PROMPT: &str = "Your session was reopened. Call next_step of the atlas MCP server to find your step, check the real state of the folder (git status, git log), then resume where the work stopped.";

/// Standing rules handed to every session at its start: how an agent works inside Atlas, whatever the step.
const WORKING_RULES: &str = include_str!("../assets/working-rules.txt");

/// The briefs are in English whatever the master speaks: this line is what makes the agent write
/// to him in his language. Given at the start of a session and again with each step, since the
/// master can change language while a session is open.
fn language_rule() -> String {
    let language = language::name();
    format!(
        "The master reads {language}: write to him in {language} (what you say in this terminal, your questions, a plan or a report he reads). Code, commits and merge requests follow the conventions of the repository."
    )
}

fn other_workspace_line(workspace: &ExtraWorkspace) -> String {
    format!(
        "Other working folder (project {}): {}, branch {} (base {})",
        workspace.repo_name, workspace.worktree_path, workspace.branch, workspace.base_branch
    )
}

/// First prompt of a fresh session: it only sends the agent to `next_step`.
pub fn launch_prompt(agent: &AgentRecord) -> String {
    // One line, no double quote: the prompt is a command-line argument, also on Windows.
    format!(
        "You are an Atlas agent in charge of {} ({}). Now call the next_step tool of the atlas MCP server and follow its instructions.",
        to_command_line_text(&agent.ticket_ref),
        to_command_line_text(&agent.title)
    )
}

/// The other repositories of the same work, for a review that reads them together.
fn linked_repos_line(agent: &AgentRecord, since: Option<&str>) -> String {
    if agent.workspaces.is_empty() {
        return String::new();
    }
    let range = since
        .map(|since| format!(", commits since {since}"))
        .unwrap_or_default();
    let repos: Vec<String> = agent
        .workspaces
        .iter()
        .map(|workspace| {
            format!(
                "{} (branch {}, base {}{range})",
                workspace.worktree_path, workspace.branch, workspace.base_branch
            )
        })
        .collect();
    format!("The same work also touches, to review together: {}.", repos.join(" ; "))
}

/// The slash command a brick starts from, as if the master had typed it.
pub fn command_prompt(agent: &AgentRecord, step: &StepRecord, brick: &Brick, command: &str) -> String {
    let previous_pass =
        agent.steps.iter().rev().find(|other| {
            other.brick_id == step.brick_id && other.state == StepState::Done && other.head_sha.is_some()
        });
    let previous_head = previous_pass.and_then(|pass| pass.head_sha.as_deref());
    let need = agent
        .steps
        .iter()
        .rev()
        .find_map(|other| other.output.as_ref()?.get("need").filter(|need| !need.is_empty()));
    let is_later_pass = step.pass > 1;
    // Passes after the first only re-read what the correction changed.
    let range = match previous_head {
        Some(head) if is_later_pass => format!("the commits {head}..HEAD"),
        _ if agent.branch.is_empty() => "the commit HEAD".to_owned(),
        _ => format!("the branch {}", agent.branch),
    };
    let default_need = format!("{} ({})", agent.ticket_ref, agent.title);
    let fail_on: Vec<&str> = brick
        .fail_on
        .iter()
        .flatten()
        .map(|severity| severity.as_str())
        .collect();
    let pass_note = if is_later_pass {
        format!(
            "Pass {}: before reviewing, call next_step of the atlas MCP server for the findings of the previous pass, check that they are closed, and review nothing outside this range.",
            step.pass
        )
    } else {
        String::new()
    };
    let since = previous_pass
        .filter(|_| is_later_pass)
        .and_then(|pass| pass.ended_at.as_deref());
    render_command(
        command,
        &[
            ("range", &range),
            ("worktree", &agent.worktree_path),
            ("branch", &agent.branch),
            ("base", &agent.base_branch),
            ("ticket", &to_command_line_text(&agent.ticket_ref)),
            ("title", &to_command_line_text(&agent.title)),
            (
                "need",
                &to_command_line_text(need.map_or(default_need.as_str(), String::as_str)),
            ),
            ("failOn", &fail_on.join(", ")),
            ("passNote", &pass_note),
            ("repos", &linked_repos_line(agent, since)),
            ("mrUrl", agent.mr_url.as_deref().unwrap_or_default()),
        ],
    )
}

/// The sections of the agent's file a brick reads, ready to hand to its session.
fn dossier_lines(agent: &AgentRecord, brick: &Brick) -> Vec<String> {
    // The master's own words go to every step: no brick can do its work against them.
    let mut lines: Vec<String> = match agent.note.as_deref().filter(|note| !note.is_empty()) {
        Some(note) => vec!["## Note of the master".to_owned(), note.to_owned()],
        None => Vec::new(),
    };
    for section in brick.reads.iter().filter(|section| *section != NOTE_SECTION) {
        let Some(source) = agent
            .steps
            .iter()
            .rev()
            .find(|step| step.brick_id == *section && step.is_finished())
        else {
            continue;
        };
        let pass = if source.pass > 1 {
            format!(", pass {}", source.pass)
        } else {
            String::new()
        };
        lines.push(format!("## {}{pass}", source.name));
        match &source.output {
            Some(output) => lines.extend(output.iter().map(|(field, value)| format!("{field}: {value}"))),
            None => lines.push(
                "Done outside Atlas: its result is to be read where it is, according to the note of the master (the merge request, its comments, the code)."
                    .to_owned(),
            ),
        }
    }
    lines
}

/// What `next_step` hands the agent: where it stands, its file, the instructions and the fields to return.
pub fn step_brief(agent: &AgentRecord, step: &StepRecord, brick: &Brick) -> String {
    let review = looping_review(brick);
    let mut lines = vec![
        format!("Step: {}", step.name),
        format!("Workflow: {}", describe_path(agent)),
    ];
    lines.extend(agent.workspaces.iter().map(other_workspace_line));
    if let Some(mr_url) = agent.mr_url.as_deref().filter(|url| !url.is_empty()) {
        lines.push(format!("Merge request: {mr_url}"));
    }
    if agent.branch.is_empty() && brick.requires_clean_tree {
        lines.extend([String::new(), NO_BRANCH_NOTICE.to_owned()]);
    }
    if let Some(review) = review {
        let fail_on: Vec<&str> = review.fail_on.iter().map(|severity| severity.as_str()).collect();
        lines.push(format!(
            "Review pass {} of {}, threshold: {}",
            step.pass,
            review.max_passes,
            fail_on.join(", ")
        ));
    }
    let dossier = dossier_lines(agent, brick);
    if !dossier.is_empty() {
        lines.extend([String::new(), "Case file:".to_owned()]);
        lines.extend(dossier);
    }
    lines.extend([
        String::new(),
        "Instructions:".to_owned(),
        brick.instructions.trim().to_owned(),
        String::new(),
        "To hand in through fields of complete_step, once the step is done and checked:".to_owned(),
    ]);
    lines.extend(
        brick
            .writes
            .iter()
            .map(|(field, expected)| format!("- {field}: {expected}")),
    );
    if review.is_some() {
        lines.push("- and findingsAboveThreshold: the number of findings at the threshold.".to_owned());
    }
    lines.extend([String::new(), language_rule()]);
    lines.join("\n")
}

/// What earlier sessions learnt about the agent's projects, so this one does not ask again.
fn project_memory_lines(agent: &AgentRecord, global_notes: &[String], repos: &[RepoView]) -> Vec<String> {
    let mut lines = Vec::new();
    if !global_notes.is_empty() {
        lines.push("To know for all the projects, at the request of the master:".to_owned());
        lines.extend(global_notes.iter().map(|note| format!("- {note}")));
    }
    let is_agent_repo = |repo: &RepoView| {
        repo.path == agent.repo_path
            || agent
                .workspaces
                .iter()
                .any(|workspace| workspace.repo_path == repo.path)
    };
    for repo in repos
        .iter()
        .filter(|repo| is_agent_repo(repo) && !repo.notes.is_empty())
    {
        lines.push(format!(
            "To know about the project {}, learnt during earlier works:",
            repo.name
        ));
        lines.extend(repo.notes.iter().map(|note| format!("- {note}")));
    }
    lines
}

/// SessionStart context: who the session is, where it stands in its workflow, its projects' memory and the standing rules.
pub fn session_start_brief(
    agent: &AgentRecord,
    current: Option<(&StepRecord, &Brick)>,
    global_notes: &[String],
    repos: &[RepoView],
) -> String {
    let folder = if agent.branch.is_empty() {
        format!(
            "Working folder: {}, without a branch for now (base {}). Nothing is committed there; work_on_branch settles you on an existing branch if the work must change code.",
            agent.worktree_path, agent.base_branch
        )
    } else {
        format!(
            "Working folder: {}, branch {} (base {}).",
            agent.worktree_path, agent.branch, agent.base_branch
        )
    };
    let mut lines = vec![
        format!(
            "You are the Atlas agent in charge of {} ({}).",
            agent.ticket_ref, agent.title
        ),
        folder,
    ];
    lines.extend(
        agent
            .workspaces
            .iter()
            .map(|workspace| format!("{}.", other_workspace_line(workspace))),
    );
    lines.push(format!("Your workflow: {}", describe_path(agent)));
    lines.extend(project_memory_lines(agent, global_notes, repos));
    if let Some((step, brick)) = current {
        lines.push(format!(
            "This session serves the step {}. Other sessions do the others: you only have the case file handed by next_step.",
            step.name
        ));
        if brick.command.is_some() {
            lines.push(
                "It starts with a command: once its work is done, call next_step of the atlas MCP server, then complete_step."
                    .to_owned(),
            );
        }
    }
    lines.push(WORKING_RULES.trim_end().to_owned());
    lines.push(language_rule());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::tests::shipped_catalogue;
    use crate::fixtures::{assert_fixture, detached_agent, review_agent, sample_repos, step};

    #[test]
    fn sends_a_fresh_session_to_next_step_on_one_line_without_double_quotes() {
        assert_fixture(&launch_prompt(&review_agent()), "launch-prompt.txt");
    }

    #[test]
    fn briefs_a_later_review_pass_with_its_file() {
        let (agent, catalogue) = (review_agent(), shipped_catalogue());
        assert_fixture(
            &step_brief(&agent, &agent.steps[5], &catalogue["review"]),
            "step-brief-review.txt",
        );
    }

    #[test]
    fn briefs_a_step_that_commits_without_a_branch_with_the_notice() {
        let (agent, catalogue) = (detached_agent(), shipped_catalogue());
        assert_fixture(
            &step_brief(&agent, &agent.steps[0], &catalogue["implementation"]),
            "step-brief-detached.txt",
        );
    }

    #[test]
    fn starts_a_later_pass_from_the_fix_commits_only() {
        let (agent, catalogue) = (review_agent(), shipped_catalogue());
        let review = &catalogue["review"];
        let command = review.command.as_deref().expect("command");
        assert_fixture(
            &command_prompt(&agent, &agent.steps[5], review, command),
            "command-prompt-pass2.txt",
        );
    }

    #[test]
    fn starts_a_first_pass_without_a_branch_from_the_head_commit() {
        let (agent, catalogue) = (detached_agent(), shipped_catalogue());
        let review = &catalogue["review"];
        let first_pass = step("p1", "review", "Review", StepState::Current, "opus");
        let command = review.command.as_deref().expect("command");
        assert_fixture(
            &command_prompt(&agent, &first_pass, review, command),
            "command-prompt-pass1.txt",
        );
    }

    #[test]
    fn tells_a_session_who_it_is_with_the_memory_of_its_projects_only() {
        let (agent, catalogue) = (review_agent(), shipped_catalogue());
        let brief = session_start_brief(
            &agent,
            Some((&agent.steps[5], &catalogue["review"])),
            &["Commits en anglais".to_owned()],
            &sample_repos(),
        );
        assert_fixture(&brief, "session-start-review.txt");
    }

    #[test]
    fn tells_a_session_without_a_branch_how_to_get_one() {
        assert_fixture(
            &session_start_brief(&detached_agent(), None, &[], &[]),
            "session-start-detached.txt",
        );
    }
}
