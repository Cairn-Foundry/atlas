// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What ties the daemon to Claude Code: the environment of a session, the trust of a repository,
//! the files a session is started with and the transcripts its usage is read from.

use crate::language::tr;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::contract::StepUsage;
use crate::error::{Error, Result};

/// Variables a running Claude Code session sets for its own children. Inherited by an agent or the
/// orchestrator (when the daemon itself was started from a Claude Code session), they turn the new
/// `claude` into a child session that keeps no transcript, so `--resume` later finds nothing.
const SESSION_SCOPED: [&str; 11] = [
    "CLAUDECODE",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_CHILD_NOTES_DIR",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
];

/// A copy of `source` fit to start an independent Claude Code session, plus `extra`.
pub fn claude_env(source: &[(String, String)], extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = source
        .iter()
        .filter(|(key, _)| !SESSION_SCOPED.contains(&key.as_str()) && !extra.iter().any(|(name, _)| name == key))
        .cloned()
        .collect();
    env.extend(
        extra
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
    );
    env
}

/// The environment of this process, without the variables that are not valid text.
pub fn process_env() -> Vec<(String, String)> {
    std::env::vars_os()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .collect()
}

/// Where Claude Code keeps its files: `CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn claude_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::home_dir().unwrap_or_default().join(".claude"))
}

/// Claude Code's global state file, which records the folders the user trusts.
pub fn claude_state_path() -> PathBuf {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir).join(".claude.json"),
        None => std::env::home_dir().unwrap_or_default().join(".claude.json"),
    }
}

fn object(value: Option<&Value>) -> Map<String, Value> {
    value.and_then(Value::as_object).cloned().unwrap_or_default()
}

/// Marks a repository as trusted so Claude Code skips the "trust this folder" dialog in the agents'
/// worktrees: Claude Code files the trust of a git worktree under its main repository.
/// Returns false when the folder was already trusted (nothing written).
///
/// The state file is shared with running Claude Code processes, so the write is atomic and only
/// happens when the flag is missing, typically once per repository.
pub fn trust_repository(repo_root: &str, state_path: &Path) -> Result<bool> {
    let state: Value = if state_path.exists() {
        serde_json::from_str(&std::fs::read_to_string(state_path)?)?
    } else {
        json!({})
    };
    let Some(state) = state.as_object() else {
        return Err(Error::internal(tr!(
            "{} : objet JSON attendu",
            "{}: JSON object expected",
            state_path.display()
        )));
    };
    let mut projects = object(state.get("projects"));
    let mut project = object(projects.get(repo_root));
    if project.get("hasTrustDialogAccepted") == Some(&Value::Bool(true)) {
        return Ok(false);
    }
    project.insert("hasTrustDialogAccepted".to_owned(), Value::Bool(true));
    projects.insert(repo_root.to_owned(), Value::Object(project));
    let mut next = state.clone();
    next.insert("projects".to_owned(), Value::Object(projects));

    let mut temporary = state_path.as_os_str().to_owned();
    // Two launches at once each write their own file.
    temporary.push(format!(".atlas-{}.tmp", uuid::Uuid::new_v4().simple()));
    let temporary = PathBuf::from(temporary);
    write_secret(&temporary, &serde_json::to_string_pretty(&Value::Object(next))?)?;
    std::fs::rename(&temporary, state_path)?;
    Ok(true)
}

/// Writes a file that holds a secret, readable by its owner only from the moment it exists.
pub fn write_secret(path: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        // A file that already existed keeps its mode: it is narrowed before the secret is written.
        if path.exists() {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    options.open(path)?.write_all(content.as_bytes())
}

/// Hook events Atlas listens to, as Claude Code names them, with the argument the hook command takes.
const AGENT_HOOKS: [(&str, &str, Option<&str>); 5] = [
    ("Stop", "stop", None),
    ("Notification", "notification", Some("idle_prompt")),
    ("PostToolUse", "post-tool-use", None),
    ("SessionStart", "session-start", None),
    ("UserPromptSubmit", "user-prompt-submit", None),
];

/// Arguments accepted by the `hook` command of the daemon, one per hook event.
pub const HOOK_EVENTS: [&str; 5] = [
    "stop",
    "notification",
    "post-tool-use",
    "session-start",
    "user-prompt-submit",
];

fn write_json(path: &Path, value: &Value) -> Result<()> {
    Ok(std::fs::write(path, serde_json::to_string_pretty(value)?)?)
}

/// Points a Claude Code session at one MCP endpoint of the daemon, as the `atlas` server.
pub fn write_atlas_mcp_config(path: &Path, url: &str, token: &str) -> Result<()> {
    let authorization = format!("Bearer {token}");
    let config = json!({ "mcpServers": { "atlas": { "type": "http", "url": url, "headers": { "Authorization": authorization } } } });
    Ok(write_secret(path, &serde_json::to_string_pretty(&config)?)?)
}

/// Settings of an agent session: its hooks go to the `hook` command of `daemon_executable`, and the
/// warning of bypassPermissions is skipped, since nobody is at the terminal to accept it and its
/// default answer exits.
pub fn write_agent_settings(path: &Path, daemon_executable: &str) -> Result<()> {
    let executable = daemon_executable.replace('\\', "/");
    let mut hooks = Map::new();
    for (event, argument, matcher) in AGENT_HOOKS {
        let mut entry = Map::new();
        if let Some(matcher) = matcher {
            entry.insert("matcher".to_owned(), json!(matcher));
        }
        entry.insert(
            "hooks".to_owned(),
            json!([{ "type": "command", "command": format!("\"{executable}\" hook {argument}") }]),
        );
        hooks.insert(event.to_owned(), json!([entry]));
    }
    write_json(
        path,
        &json!({ "hooks": hooks, "skipDangerousModePermissionPrompt": true }),
    )
}

fn copy_folder(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Writes in `dir`, from scratch, the plugin that gives a session the named skills of `skills_dir`
/// and subagents of `subagents_dir`: a plugin loads for one session only, so an agent gets what its
/// brick names and nothing else of the master's collection. Claude Code calls the subagents
/// `atlas:<name>`; a skill keeps its slash command, and finds its own files under
/// `${CLAUDE_PLUGIN_ROOT}`.
pub fn write_session_plugin(
    dir: &Path,
    skills_dir: &Path,
    skills: &[String],
    subagents_dir: &Path,
    subagents: &[String],
) -> Result<()> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir.join(".claude-plugin"))?;
    write_json(&dir.join(".claude-plugin/plugin.json"), &json!({ "name": "atlas" }))?;
    for skill in skills {
        copy_folder(&skills_dir.join(skill), &dir.join("skills").join(skill)).map_err(|error| {
            Error::refusal(tr!(
                "skill {skill} illisible dans {} : {error}",
                "skill {skill} unreadable in {}: {error}",
                skills_dir.display()
            ))
        })?;
    }
    if !subagents.is_empty() {
        std::fs::create_dir_all(dir.join("agents"))?;
    }
    for subagent in subagents {
        let file = format!("{subagent}.md");
        std::fs::copy(subagents_dir.join(&file), dir.join("agents").join(&file)).map_err(|error| {
            Error::refusal(tr!(
                "sous-agent {subagent} illisible dans {} : {error}",
                "subagent {subagent} unreadable in {}: {error}",
                subagents_dir.display()
            ))
        })?;
    }
    Ok(())
}

fn count(usage: &Value, key: &str) -> u64 {
    usage.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// The usage carried by one transcript line, none for anything that is not an assistant message.
fn parse_usage_line(line: &str) -> Option<(String, String, StepUsage)> {
    let entry: Value = serde_json::from_str(line).ok()?;
    if entry.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let timestamp = entry.get("timestamp")?.as_str()?;
    let message = entry.get("message")?;
    let usage = message.get("usage").filter(|usage| usage.is_object())?;
    Some((
        message.get("id")?.as_str()?.to_owned(),
        timestamp.to_owned(),
        StepUsage {
            input_tokens: count(usage, "input_tokens"),
            output_tokens: count(usage, "output_tokens"),
            cache_read_tokens: count(usage, "cache_read_input_tokens"),
            cache_write_tokens: count(usage, "cache_creation_input_tokens"),
        },
    ))
}

/// Sums the usage of the assistant messages written between `from` and `to` (ISO dates).
/// A message spans several lines, one per content block, each repeating its usage: it counts once.
pub fn sum_usage<'a>(transcripts: impl IntoIterator<Item = &'a str>, from: &str, to: &str) -> StepUsage {
    let mut by_message: HashMap<String, StepUsage> = HashMap::new();
    for transcript in transcripts {
        for (message_id, timestamp, usage) in transcript.lines().filter_map(parse_usage_line) {
            if timestamp.as_str() >= from && timestamp.as_str() <= to {
                by_message.insert(message_id, usage);
            }
        }
    }
    by_message
        .values()
        .fold(StepUsage::default(), |total, usage| StepUsage {
            input_tokens: total.input_tokens + usage.input_tokens,
            output_tokens: total.output_tokens + usage.output_tokens,
            cache_read_tokens: total.cache_read_tokens + usage.cache_read_tokens,
            cache_write_tokens: total.cache_write_tokens + usage.cache_write_tokens,
        })
}

/// Transcript of a session and of the subagents it launched, wherever Claude Code filed them.
fn transcript_files(claude_dir: &Path, session_id: &str) -> Vec<PathBuf> {
    let Ok(projects) = std::fs::read_dir(claude_dir.join("projects")) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for project in projects.flatten().map(|entry| entry.path()) {
        let main = project.join(format!("{session_id}.jsonl"));
        if main.exists() {
            files.push(main);
        }
        if let Ok(subagents) = std::fs::read_dir(project.join(session_id).join("subagents")) {
            files.extend(
                subagents
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl")),
            );
        }
    }
    files
}

/// What a session consumed between two dates according to Claude Code's transcripts; none when there is none.
pub fn read_usage(claude_dir: &Path, session_id: &str, from: &str, to: &str) -> Option<StepUsage> {
    let transcripts: Vec<String> = transcript_files(claude_dir, session_id)
        .iter()
        .filter_map(|file| std::fs::read_to_string(file).ok())
        .collect();
    if transcripts.is_empty() {
        return None;
    }
    Some(sum_usage(transcripts.iter().map(String::as_str), from, to))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(values: &[(&str, &str)]) -> Vec<(String, String)> {
        values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn drops_the_variables_of_the_session_the_daemon_was_started_from() {
        let source = pairs(&[
            ("PATH", "/bin"),
            ("CLAUDECODE", "1"),
            ("CLAUDE_CODE_SESSION_ID", "abc"),
            ("ATLAS_URL", "old"),
        ]);
        assert_eq!(
            claude_env(&source, &[("ATLAS_URL", "new")]),
            pairs(&[("PATH", "/bin"), ("ATLAS_URL", "new")])
        );
    }

    #[test]
    fn trusts_a_repository_once_and_keeps_the_rest_of_the_state() {
        let dir = tempfile::tempdir().expect("dir");
        let state_path = dir.path().join(".claude.json");
        std::fs::write(
            &state_path,
            r#"{"theme":"dark","projects":{"/repo":{"allowedTools":["Bash"]}}}"#,
        )
        .expect("state");
        assert!(trust_repository("/repo", &state_path).expect("first"));
        assert!(!trust_repository("/repo", &state_path).expect("second"));
        let state: Value = serde_json::from_str(&std::fs::read_to_string(&state_path).expect("read")).expect("json");
        assert_eq!(
            state,
            json!({ "theme": "dark", "projects": { "/repo": { "allowedTools": ["Bash"], "hasTrustDialogAccepted": true } } })
        );
        assert_eq!(std::fs::read_dir(dir.path()).expect("list").count(), 1);
    }

    #[test]
    fn creates_the_state_file_when_claude_code_never_ran_and_refuses_one_that_is_not_an_object() {
        let dir = tempfile::tempdir().expect("dir");
        let state_path = dir.path().join(".claude.json");
        assert!(trust_repository("/repo", &state_path).expect("created"));
        std::fs::write(&state_path, "[]").expect("array");
        assert!(trust_repository("/repo", &state_path).is_err());
    }

    #[test]
    fn writes_the_files_a_session_is_started_with() {
        let dir = tempfile::tempdir().expect("dir");
        let read = |name: &str| -> Value {
            serde_json::from_str(&std::fs::read_to_string(dir.path().join(name)).expect("read")).expect("json")
        };
        write_atlas_mcp_config(
            &dir.path().join("mcp.json"),
            "http://127.0.0.1:4317/mcp/agent/a1",
            "secret",
        )
        .expect("mcp");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("mcp.json"))
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "the config holds the shared secret");
        }
        assert_eq!(
            read("mcp.json"),
            json!({ "mcpServers": { "atlas": { "type": "http", "url": "http://127.0.0.1:4317/mcp/agent/a1", "headers": { "Authorization": "Bearer secret" } } } })
        );
        write_agent_settings(&dir.path().join("settings.json"), "C:\\Atlas\\atlas-daemon.exe").expect("settings");
        let settings = read("settings.json");
        assert_eq!(settings["skipDangerousModePermissionPrompt"], true);
        assert_eq!(
            settings["hooks"]["Stop"],
            json!([{ "hooks": [{ "type": "command", "command": "\"C:/Atlas/atlas-daemon.exe\" hook stop" }] }])
        );
        assert_eq!(settings["hooks"]["Notification"][0]["matcher"], "idle_prompt");
        let arguments: Vec<&str> = AGENT_HOOKS.iter().map(|(_, argument, _)| *argument).collect();
        assert_eq!(arguments, HOOK_EVENTS);
    }

    #[test]
    fn gives_a_session_the_named_skills_and_subagents_only() {
        let home = tempfile::tempdir().expect("home");
        let (skills, subagents) = (home.path().join("skills"), home.path().join("subagents"));
        for skill in ["deep-review", "other"] {
            std::fs::create_dir_all(skills.join(skill).join("references")).expect("skill");
            std::fs::write(skills.join(skill).join("SKILL.md"), skill).expect("SKILL.md");
            std::fs::write(skills.join(skill).join("references/method.md"), "method").expect("reference");
        }
        std::fs::create_dir_all(&subagents).expect("subagents");
        std::fs::write(subagents.join("minos.md"), "minos").expect("minos");
        let plugin = home.path().join("agent/plugin");
        std::fs::create_dir_all(plugin.join("skills/stale")).expect("stale");

        let named = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>();
        write_session_plugin(
            &plugin,
            &skills,
            &named(&["deep-review"]),
            &subagents,
            &named(&["minos"]),
        )
        .expect("plugin");
        let manifest: Value = serde_json::from_str(
            &std::fs::read_to_string(plugin.join(".claude-plugin/plugin.json")).expect("manifest"),
        )
        .expect("json");
        assert_eq!(manifest, json!({ "name": "atlas" }));
        assert!(plugin.join("skills/deep-review/references/method.md").is_file());
        assert!(plugin.join("agents/minos.md").is_file());
        assert!(!plugin.join("skills/other").exists() && !plugin.join("skills/stale").exists());

        let error = write_session_plugin(&plugin, &skills, &named(&["ghost"]), &subagents, &[]).expect_err("missing");
        assert!(error.to_string().contains("skill ghost"), "{error}");
    }

    const TRANSCRIPT: &str = concat!(
        r#"{"type":"user","timestamp":"2026-01-01T10:00:00.000Z","message":{"id":"u1"}}"#,
        "\n",
        r#"{"type":"assistant","timestamp":"2026-01-01T10:01:00.000Z","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100,"cache_creation_input_tokens":7}}}"#,
        "\n",
        r#"{"type":"assistant","timestamp":"2026-01-01T10:01:01.000Z","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":8,"cache_read_input_tokens":100,"cache_creation_input_tokens":7}}}"#,
        "\nnot json\n",
        r#"{"type":"assistant","timestamp":"2026-01-01T12:00:00.000Z","message":{"id":"m2","usage":{"input_tokens":1000,"output_tokens":1000}}}"#,
        "\n"
    );

    #[test]
    fn counts_each_message_once_within_the_dates() {
        let usage = sum_usage([TRANSCRIPT], "2026-01-01T10:00:00.000Z", "2026-01-01T11:00:00.000Z");
        assert_eq!(
            usage,
            StepUsage {
                input_tokens: 10,
                output_tokens: 8,
                cache_read_tokens: 100,
                cache_write_tokens: 7
            }
        );
    }

    #[test]
    fn reads_the_transcripts_of_a_session_and_its_subagents() {
        let dir = tempfile::tempdir().expect("dir");
        let project = dir.path().join("projects/-repo");
        std::fs::create_dir_all(project.join("s1/subagents")).expect("dirs");
        std::fs::write(project.join("s1.jsonl"), TRANSCRIPT).expect("main");
        std::fs::write(
            project.join("s1/subagents/agent-a.jsonl"),
            TRANSCRIPT.replace("\"m1\"", "\"sub1\""),
        )
        .expect("subagent");
        let usage =
            read_usage(dir.path(), "s1", "2026-01-01T10:00:00.000Z", "2026-01-01T11:00:00.000Z").expect("usage");
        assert_eq!((usage.input_tokens, usage.output_tokens), (20, 16));
        assert_eq!(read_usage(dir.path(), "unknown", "", "9"), None);
    }
}
