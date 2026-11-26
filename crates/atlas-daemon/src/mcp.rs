// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The two MCP servers of the daemon, served over stateless Streamable HTTP with JSON answers:
//! the workflow tools an agent pulls its work from, and the orchestrator's hands on the factory.
//!
//! The tools and their input schemas are the published contract in `assets/*-tools.json`.

use std::sync::LazyLock;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::contract::{CatalogueChange, CreateAgentInput, DAEMON_VERSION, RepoInput, StepState, inline_issues};
use crate::error::{Error, Result, refuse};
use crate::language::tr;
use crate::service::{AgentService, CompleteStepInput};
use crate::store::{AgentRecord, Store, to_agent_view};

const SERVER_NAME: &str = "atlas";
/// Protocol revision answered to a client that names none.
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_REQUEST: i32 = -32600;
const INTERNAL_ERROR: i32 = -32603;
const DEFAULT_TERMINAL_LINES: usize = 40;
const TERMINAL_LINES: std::ops::RangeInclusive<usize> = 5..=200;

fn parse_tools(source: &str) -> Value {
    serde_json::from_str(source).expect("the tool definitions shipped with the daemon are valid JSON")
}

static AGENT_TOOLS: LazyLock<Value> = LazyLock::new(|| parse_tools(include_str!("../assets/agent-tools.json")));
static ORCHESTRATOR_TOOLS: LazyLock<Value> =
    LazyLock::new(|| parse_tools(include_str!("../assets/orchestrator-tools.json")));

#[derive(Clone)]
pub enum McpServer {
    /// The workflow tools of one agent, bound to it by the URL.
    Agent { service: AgentService, agent_id: String },
    /// Everything the orchestrator may do to the factory.
    Orchestrator { service: AgentService, store: Store },
}

/// The arguments of a tool call, read as the tool expects them; a mismatch is a refusal the model can read.
fn arguments<T: DeserializeOwned>(tool: &str, arguments: Value) -> Result<T> {
    serde_json::from_value(arguments)
        .map_err(|error| Error::refusal(format!("Invalid arguments for tool {tool}: {error}")))
}

fn required(field: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        refuse!("{field} : texte vide", "{field}: empty text");
    }
    Ok(())
}

fn to_json(value: &impl serde::Serialize) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

/// The agent an orchestrator tool names, refused in words the orchestrator can act on when it is unknown.
fn require_agent(store: &Store, agent_id: &str) -> Result<AgentRecord> {
    match store.get_agent(agent_id)? {
        Some(agent) => Ok(agent),
        None => refuse!("agent inconnu : {agent_id}", "unknown agent: {agent_id}"),
    }
}

/// One agent on one line: tool results are re-read on every turn of the orchestrator, so they stay small.
pub fn describe_agent_line(agent: &AgentRecord, is_session_open: bool) -> String {
    let path: Vec<String> = agent
        .steps
        .iter()
        .filter(|step| !matches!(step.state, StepState::Skipped | StepState::External))
        .map(|step| {
            if step.state == StepState::Current {
                format!("[{}]", step.brick_id)
            } else {
                step.brick_id.clone()
            }
        })
        .collect();
    let reason = agent
        .status_reason
        .as_deref()
        .filter(|reason| !reason.is_empty())
        .map(|reason| format!(" ({reason})"))
        .unwrap_or_default();
    format!(
        "{} | {} | project {} | {}{reason} | session {} | {} | agentId {}",
        agent.ticket_ref,
        agent.title,
        agent.repo_name,
        agent.status.as_str(),
        if is_session_open { "open" } else { "closed" },
        path.join(" > "),
        agent.id
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkOnBranch {
    branch: String,
    base_branch: Option<String>,
}

#[derive(Deserialize)]
struct RequestRepo {
    repo: String,
    reason: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProposeNote {
    note: String,
    repo: Option<String>,
    all_projects: Option<bool>,
}

#[derive(Deserialize)]
struct AskUser {
    question: String,
}

#[derive(Deserialize)]
struct Reason {
    reason: String,
}

async fn call_agent_tool(service: &AgentService, agent_id: &str, tool: &str, input: Value) -> Result<String> {
    match tool {
        "next_step" => service.next_step_when_ready(agent_id).await,
        "complete_step" => {
            service
                .complete_step(agent_id, arguments::<CompleteStepInput>(tool, input)?)
                .await
        }
        "work_on_branch" => {
            let input: WorkOnBranch = arguments(tool, input)?;
            required("branch", &input.branch)?;
            service
                .work_on_branch(agent_id, &input.branch, input.base_branch.as_deref())
                .await
        }
        "request_repo" => {
            let input: RequestRepo = arguments(tool, input)?;
            required("repo", &input.repo)?;
            required("reason", &input.reason)?;
            service.request_repo(agent_id, &input.repo, &input.reason).await
        }
        "propose_note" => {
            let input: ProposeNote = arguments(tool, input)?;
            service.propose_note(
                agent_id,
                &input.note,
                input.repo.as_deref(),
                input.all_projects.unwrap_or(false),
            )
        }
        "ask_user" => {
            let input: AskUser = arguments(tool, input)?;
            required("question", &input.question)?;
            service.ask_user(agent_id, &input.question)
        }
        "block" => {
            let input: Reason = arguments(tool, input)?;
            required("reason", &input.reason)?;
            service.block(agent_id, &input.reason)
        }
        _ => Err(Error::refusal(format!("MCP error -32602: Tool {tool} not found"))),
    }
}

#[derive(Deserialize)]
struct RepoNote {
    repo: Option<String>,
    note: String,
}

#[derive(Deserialize)]
struct RepoNotes {
    repo: Option<String>,
    notes: Vec<String>,
}

#[derive(Deserialize)]
struct RepoSetup {
    repo: String,
    command: Option<String>,
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentOnly {
    agent_id: String,
}

fn approved_by_default() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApproveStep {
    agent_id: String,
    #[serde(default = "approved_by_default")]
    approved: bool,
    reason: String,
    steps: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangeWorkflow {
    agent_id: String,
    steps: Vec<String>,
    reason: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StopAgent {
    agent_id: String,
    reason: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResumeAgent {
    agent_id: String,
    prompt: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadTerminal {
    agent_id: String,
    lines: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendToAgent {
    agent_id: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NotifyMaster {
    text: String,
    agent_id: Option<String>,
}

#[derive(Deserialize)]
struct ProposeCatalogueChange {
    change: CatalogueChange,
    reason: String,
}

#[derive(Deserialize)]
struct Broadcast {
    text: String,
}

async fn call_orchestrator_tool(service: &AgentService, store: &Store, tool: &str, input: Value) -> Result<String> {
    let projects = service.projects();
    let line = |agent: &AgentRecord| describe_agent_line(agent, service.has_live_session(&agent.id));
    match tool {
        "add_repo_note" => {
            let input: RepoNote = arguments(tool, input)?;
            match input.repo.as_deref().filter(|repo| !repo.is_empty()) {
                Some(repo) => to_json(&projects.add_note(repo, &input.note)?.notes),
                None => to_json(&projects.add_global_note(&input.note)?),
            }
        }
        "set_repo_notes" => {
            let input: RepoNotes = arguments(tool, input)?;
            match input.repo.as_deref().filter(|repo| !repo.is_empty()) {
                Some(repo) => to_json(&projects.set_notes(repo, &input.notes)?.notes),
                None => to_json(&projects.set_global_notes(&input.notes)?),
            }
        }
        "set_repo_setup" => {
            let input: RepoSetup = arguments(tool, input)?;
            to_json(&projects.set_setup(&input.repo, input.command.as_deref())?)
        }
        "list_repos" => {
            to_json(&json!({ "notesForAllProjects": projects.list_global_notes()?, "repos": projects.list()? }))
        }
        "add_repo" => to_json(&projects.add(arguments::<RepoInput>(tool, input)?).await?),
        "remove_repo" => {
            let input: Named = arguments(tool, input)?;
            projects.remove(&input.name)?;
            Ok(format!("Project {} removed.", input.name))
        }
        "list_agents" => {
            let lines: Vec<String> = store.list_agents(false)?.iter().map(line).collect();
            Ok(if lines.is_empty() {
                "No agent.".to_owned()
            } else {
                lines.join("\n")
            })
        }
        "get_agent" => {
            let input: AgentOnly = arguments(tool, input)?;
            let agent = require_agent(store, &input.agent_id)?;
            let mut detail = serde_json::to_value(to_agent_view(&agent))?;
            if let Some(fields) = detail.as_object_mut() {
                // The report is for the master: the orchestrator reads the step fields it needs elsewhere, not twice.
                fields.remove("report");
                fields.insert("note".to_owned(), json!(agent.note));
                fields.insert(
                    "sessions".to_owned(),
                    serde_json::to_value(store.list_sessions(&agent.id)?)?,
                );
            }
            to_json(&detail)
        }
        "launch_agent" => {
            let input = arguments::<CreateAgentInput>(tool, input)?
                .validated()
                .map_err(|issues| Error::refusal(inline_issues(&issues, true)))?;
            if input.steps.is_none() {
                refuse!("steps : le parcours est requis", "steps: the workflow is required");
            }
            if input.reason.is_none() {
                refuse!(
                    "reason : la raison du parcours est requise",
                    "reason: the reason for the workflow is required"
                );
            }
            let agent = service.launch(input).await?;
            Ok(format!("Launched. {}", describe_agent_line(&agent, true)))
        }
        "approve_step" => {
            let input: ApproveStep = arguments(tool, input)?;
            required("reason", &input.reason)?;
            let agent = if input.approved {
                service.approve_step(&input.agent_id, &input.reason, input.steps.as_deref())?
            } else {
                service.reject_step(&input.agent_id, &input.reason)?
            };
            Ok(format!("Decision recorded. {}", line(&agent)))
        }
        "set_workflow" => {
            let input: ChangeWorkflow = arguments(tool, input)?;
            required("reason", &input.reason)?;
            Ok(format!(
                "Workflow changed. {}",
                line(&service.set_workflow(&input.agent_id, &input.steps, &input.reason)?)
            ))
        }
        "stop_agent" => {
            let input: StopAgent = arguments(tool, input)?;
            required("reason", &input.reason)?;
            require_agent(store, &input.agent_id)?;
            Ok(format!(
                "Agent stopped. {}",
                describe_agent_line(&service.stop(&input.agent_id, &input.reason)?, false)
            ))
        }
        "jump_to_step" => {
            let input: ChangeWorkflow = arguments(tool, input)?;
            required("reason", &input.reason)?;
            if input.steps.is_empty() {
                refuse!("steps : au moins une brique", "steps: at least one brick");
            }
            require_agent(store, &input.agent_id)?;
            Ok(format!(
                "Agent moved on. {}",
                line(&service.jump_to(&input.agent_id, &input.steps, &input.reason)?)
            ))
        }
        "archive_agent" => {
            let input: AgentOnly = arguments(tool, input)?;
            require_agent(store, &input.agent_id)?;
            service.archive(&input.agent_id)?;
            Ok("Agent archived.".to_owned())
        }
        "resume_agent" => {
            let input: ResumeAgent = arguments(tool, input)?;
            require_agent(store, &input.agent_id)?;
            Ok(format!(
                "Session reopened. {}",
                describe_agent_line(&service.resume(&input.agent_id, input.prompt)?, true)
            ))
        }
        "restart_agent" => {
            let input: AgentOnly = arguments(tool, input)?;
            require_agent(store, &input.agent_id)?;
            Ok(format!(
                "New session. {}",
                describe_agent_line(&service.restart(&input.agent_id)?, true)
            ))
        }
        "read_agent_terminal" => {
            let input: ReadTerminal = arguments(tool, input)?;
            let lines = input.lines.unwrap_or(DEFAULT_TERMINAL_LINES);
            if !TERMINAL_LINES.contains(&lines) {
                refuse!(
                    "lines : entre {} et {}",
                    "lines: between {} and {}",
                    TERMINAL_LINES.start(),
                    TERMINAL_LINES.end()
                );
            }
            require_agent(store, &input.agent_id)?;
            let (command_line, screen) = service.read_terminal(&input.agent_id, lines)?;
            let screen = if screen.is_empty() { "(empty screen)" } else { &screen };
            Ok(format!(
                "Launch command:\n{command_line}\n\nEnd of the screen:\n{screen}"
            ))
        }
        "send_to_agent" => {
            let input: SendToAgent = arguments(tool, input)?;
            required("text", &input.text)?;
            require_agent(store, &input.agent_id)?;
            Ok(format!(
                "Message delivered. {}",
                line(&service.send_to_agent(&input.agent_id, &input.text)?)
            ))
        }
        "propose_catalogue_change" => {
            let input: ProposeCatalogueChange = arguments(tool, input)?;
            service.propose_catalogue_change(input.change, &input.reason)
        }
        "notify_master" => {
            let input: NotifyMaster = arguments(tool, input)?;
            required("text", &input.text)?;
            service.warn_master(&input.text, input.agent_id.as_deref())?;
            Ok("Master warned.".to_owned())
        }
        "broadcast" => {
            let input: Broadcast = arguments(tool, input)?;
            required("text", &input.text)?;
            Ok(format!(
                "Message broadcast to {} agent(s).",
                service.broadcast(&input.text)?
            ))
        }
        _ => Err(Error::refusal(format!("MCP error -32602: Tool {tool} not found"))),
    }
}

impl McpServer {
    fn tools(&self) -> &'static Value {
        match self {
            Self::Agent { .. } => &AGENT_TOOLS,
            Self::Orchestrator { .. } => &ORCHESTRATOR_TOOLS,
        }
    }

    async fn call_tool(&self, tool: &str, input: Value) -> Result<String> {
        match self {
            Self::Agent { service, agent_id } => call_agent_tool(service, agent_id, tool, input).await,
            Self::Orchestrator { service, store } => call_orchestrator_tool(service, store, tool, input).await,
        }
    }

    /// The answer to one JSON-RPC message; none for a notification, which expects no answer.
    async fn answer(&self, message: &Value) -> Option<Value> {
        let id = message.get("id").filter(|id| !id.is_null())?.clone();
        let failure =
            |code: i32, text: String| json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": text } });
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return Some(failure(INVALID_REQUEST, "Invalid Request".to_owned()));
        };
        let params = message.get("params");
        let result = match method {
            "initialize" => {
                let version = params
                    .and_then(|params| params.get("protocolVersion"))
                    .and_then(Value::as_str);
                json!({
                    "protocolVersion": version.unwrap_or(DEFAULT_PROTOCOL_VERSION),
                    "capabilities": { "tools": { "listChanged": true } },
                    "serverInfo": { "name": SERVER_NAME, "version": DAEMON_VERSION },
                })
            }
            "ping" => json!({}),
            "tools/list" => json!({ "tools": self.tools() }),
            "tools/call" => {
                let tool = params
                    .and_then(|params| params.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let input = params
                    .and_then(|params| params.get("arguments"))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match self.call_tool(tool, input).await {
                    Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
                    // An assert_fixture refusal is a tool error the model can read; anything else is a bug and surfaces as one.
                    Err(Error::Refusal(text)) => {
                        json!({ "content": [{ "type": "text", "text": text }], "isError": true })
                    }
                    Err(Error::Internal(text)) => {
                        tracing::error!("{}", tr!("outil {tool} : {text}", "tool {tool}: {text}"));
                        return Some(failure(INTERNAL_ERROR, text));
                    }
                }
            }
            _ => return Some(failure(METHOD_NOT_FOUND, "Method not found".to_owned())),
        };
        Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }

    /// Handles the body of one POST: a message or a batch of them. None when nothing expects an answer.
    pub async fn handle(&self, body: Value) -> Option<Value> {
        let Value::Array(messages) = body else {
            return self.answer(&body).await;
        };
        let mut answers = Vec::new();
        for message in &messages {
            answers.extend(self.answer(message).await);
        }
        (!answers.is_empty()).then_some(Value::Array(answers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{assert_fixture, review_agent};

    #[test]
    fn describes_an_agent_on_one_line_as_the_orchestrator_reads_it() {
        let agent = AgentRecord {
            status: crate::contract::AgentStatus::Blocked,
            status_reason: Some("arrêté : test".to_owned()),
            ..review_agent()
        };
        assert_fixture(&describe_agent_line(&agent, false), "agent-line.txt");
    }

    #[test]
    fn ships_every_tool_with_a_description_and_an_object_schema() {
        for (tools, count) in [(&*AGENT_TOOLS, 7), (&*ORCHESTRATOR_TOOLS, 21)] {
            let tools = tools.as_array().expect("array");
            assert_eq!(tools.len(), count);
            for tool in tools {
                assert!(tool["name"].as_str().is_some_and(|name| !name.is_empty()));
                assert!(
                    tool["description"]
                        .as_str()
                        .is_some_and(|description| !description.is_empty())
                );
                assert_eq!(tool["inputSchema"]["type"], "object");
            }
        }
    }

    /// Calls every tool of a server with no argument: whatever it answers, it must not be "not found".
    async fn unknown_tools(server: McpServer) -> Vec<String> {
        let mut unknown = Vec::new();
        for tool in server.tools().as_array().expect("tools") {
            let name = tool["name"].as_str().expect("name");
            if let Err(Error::Refusal(message)) = server.call_tool(name, json!({})).await
                && message.ends_with("not found")
            {
                unknown.push(name.to_owned());
            }
        }
        unknown
    }

    #[tokio::test]
    async fn answers_every_tool_it_publishes() {
        let env = crate::testing::Env::new();
        let agent = env.launch_default().await;
        let for_agent = McpServer::Agent {
            service: env.service.clone(),
            agent_id: agent.id,
        };
        let for_orchestrator = McpServer::Orchestrator {
            service: env.service.clone(),
            store: env.store.clone(),
        };
        assert_eq!(unknown_tools(for_agent).await, Vec::<String>::new());
        assert_eq!(unknown_tools(for_orchestrator).await, Vec::<String>::new());
    }
}
