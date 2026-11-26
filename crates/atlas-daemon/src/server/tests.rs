// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The HTTP API and the WebSockets, as a client meets them.

use std::sync::Mutex;

use axum::body::Body;
use axum::http::header::CONTENT_TYPE;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tower::ServiceExt;

use super::*;
use crate::contract::{AgentStatus, CreateAgentInput, RepoInput};
use crate::orchestrator::{ClaudeResult, ClaudeRun, ClaudeRunner};
use crate::store::AgentRecord;
use crate::testing::{Env, TOKEN, ids, settle};

const HOST_HEADER: &str = "127.0.0.1:4317";

struct Factory {
    env: Env,
    router: Router,
    stop: Arc<Notify>,
    /// Prompts the orchestrator's model was run with.
    runs: Arc<Mutex<Vec<ClaudeRun>>>,
}

fn factory() -> Factory {
    let env = Env::new();
    let runs: Arc<Mutex<Vec<ClaudeRun>>> = Arc::default();
    let recorded = Arc::clone(&runs);
    let runner: ClaudeRunner = Arc::new(move |run| {
        recorded.lock().expect("runs").push(run);
        Box::pin(async {
            Ok(ClaudeResult {
                text: "Agent lancé.".to_owned(),
                session_id: Some("orch-1".to_owned()),
                ..ClaudeResult::default()
            })
        })
    });
    let orchestrator = Orchestrator::new(
        env.store.clone(),
        env.service.clone(),
        runner,
        &env.config,
        env.clock.clock(),
    );
    let stop = Arc::new(Notify::new());
    let router = build_router(ServerDeps {
        store: env.store.clone(),
        service: env.service.clone(),
        orchestrator,
        token: TOKEN.to_owned(),
        stop: Arc::clone(&stop),
    });
    Factory {
        env,
        router,
        stop,
        runs,
    }
}

struct Answer {
    status: StatusCode,
    body: String,
}

impl Answer {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|error| panic!("{error}: {}", self.body))
    }
}

impl Factory {
    async fn request(&self, method: Method, path: &str, host: &str, has_token: bool, body: Option<Value>) -> Answer {
        let mut request = axum::http::Request::builder()
            .method(method)
            .uri(path)
            .header(HOST, host);
        if has_token {
            request = request.header(AUTHORIZATION, format!("Bearer {TOKEN}"));
        }
        let body = match body {
            Some(body) => {
                request = request.header(CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let response = self
            .router
            .clone()
            .oneshot(request.body(body).expect("request"))
            .await
            .expect("response");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        Answer {
            status,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    async fn get(&self, path: &str) -> Answer {
        self.request(Method::GET, path, HOST_HEADER, true, None).await
    }

    async fn post(&self, path: &str, body: Option<Value>) -> Answer {
        self.request(Method::POST, path, HOST_HEADER, true, body).await
    }

    /// What a local program that does not hold the shared secret gets.
    async fn without_token(&self, method: Method, path: &str) -> Answer {
        self.request(method, path, HOST_HEADER, false, Some(json!({}))).await
    }

    async fn rpc(&self, path: &str, method: &str, params: Value) -> Value {
        let answer = self
            .post(
                path,
                Some(json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })),
            )
            .await;
        assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
        answer.json()
    }

    /// The text a tool answered, and whether it is an error the model must read.
    async fn call_tool(&self, path: &str, tool: &str, arguments: Value) -> (String, bool) {
        let answer = self
            .rpc(path, "tools/call", json!({ "name": tool, "arguments": arguments }))
            .await;
        let result = &answer["result"];
        (
            result["content"][0]["text"].as_str().expect("text").to_owned(),
            result["isError"] == true,
        )
    }

    async fn launched(&self) -> AgentRecord {
        self.env.launch_default().await
    }

    fn prompts(&self) -> Vec<String> {
        self.runs
            .lock()
            .expect("runs")
            .iter()
            .map(|run| run.prompt.clone())
            .collect()
    }
}

mod rest_api {
    use super::*;

    #[tokio::test]
    async fn returns_the_snapshot_of_bricks_agents_and_messages() {
        let factory = factory();
        factory.launched().await;
        let answer = factory.get("/api/state").await;
        assert_eq!(answer.status, StatusCode::OK);
        let body = answer.json();
        assert!(
            body["bricks"]
                .as_array()
                .expect("bricks")
                .contains(&json!({ "id": "review", "name": "Review" }))
        );
        let agent = &body["agents"][0];
        assert_eq!(
            (&agent["ticketRef"], &agent["title"], &agent["status"]),
            (&json!("PROJ-224"), &json!("Relances par e-mail"), &json!("working"))
        );
        assert_eq!(
            (&agent["steps"][0]["brickId"], &agent["steps"][0]["state"]),
            (&json!("qualification"), &json!("current"))
        );
        // The public view holds none of the daemon's bookkeeping.
        assert!(agent["steps"][0].get("output").is_none() && agent.get("stopBlocks").is_none());
        assert_eq!(agent["report"], Value::Null);
        assert_eq!(body["messages"][0]["kind"], "launch");
        assert_eq!(body["orchestratorBusy"], false);
        assert_eq!(body["language"], "fr");
    }

    #[tokio::test]
    async fn changes_the_language_of_the_factory_for_a_language_it_speaks() {
        let factory = factory();
        let unknown = factory.post("/api/language", Some(json!({ "language": "de" }))).await;
        assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
        assert_eq!(factory.get("/api/state").await.json()["language"], "fr");

        let changed = factory.post("/api/language", Some(json!({ "language": "en" }))).await;
        assert_eq!(changed.status, StatusCode::NO_CONTENT);
        assert_eq!(factory.get("/api/state").await.json()["language"], "en");
        // The refusals the page shows follow.
        let answer = factory.post("/api/agents/unknown/resume", None).await;
        assert_eq!(answer.json(), json!({ "error": "unknown agent" }));
    }

    #[tokio::test]
    async fn refuses_requests_that_do_not_target_localhost() {
        let factory = factory();
        let answer = factory
            .request(Method::GET, "/api/state", "evil.example:4317", false, None)
            .await;
        assert_eq!(
            (answer.status, answer.json()),
            (StatusCode::FORBIDDEN, json!({ "error": "hôte refusé" }))
        );
        for host in ["localhost:4317", "[::1]:4317", "127.0.0.1"] {
            assert_eq!(
                factory
                    .request(Method::GET, "/api/health", host, false, None)
                    .await
                    .status,
                StatusCode::OK
            );
        }
    }

    #[tokio::test]
    async fn refuses_what_a_page_of_another_website_sends() {
        let factory = factory();
        let agent = factory.launched().await;
        let from = |origin: &'static str, path: String| {
            let request = axum::http::Request::builder()
                .method(Method::POST)
                .uri(path)
                .header(HOST, HOST_HEADER)
                .header(AUTHORIZATION, format!("Bearer {TOKEN}"))
                .header(ORIGIN, origin);
            // A browser sends such a body across sites without asking: no preflight for plain text.
            let request = request
                .header(CONTENT_TYPE, "text/plain")
                .body(Body::from(r#"{"text":"Lance un agent"}"#))
                .expect("request");
            let router = factory.router.clone();
            async move { router.oneshot(request).await.expect("response").status() }
        };
        for origin in ["https://evil.example", "null"] {
            assert_eq!(from(origin, "/api/messages".to_owned()).await, StatusCode::FORBIDDEN);
            assert_eq!(
                from(origin, format!("/api/agents/{}/archive", agent.id)).await,
                StatusCode::FORBIDDEN
            );
        }
        settle().await;
        assert!(factory.prompts().is_empty());
        assert!(factory.env.agent(&agent.id).archived_at.is_none());
        // The page of the desktop shell, on each system and in development, is let through.
        for origin in ["tauri://localhost", "http://tauri.localhost", "http://localhost:5173"] {
            assert_eq!(
                from(origin, "/api/messages".to_owned()).await,
                StatusCode::ACCEPTED,
                "{origin}"
            );
        }
    }

    #[tokio::test]
    async fn answers_nothing_but_its_health_to_a_client_without_the_token() {
        let factory = factory();
        let agent = factory.launched().await;
        let id = &agent.id;
        let guarded = [
            (Method::GET, "/api/state".to_owned()),
            (Method::POST, "/api/agents".to_owned()),
            (Method::POST, "/api/language".to_owned()),
            (Method::GET, format!("/api/agents/{id}/sessions")),
            (Method::POST, format!("/api/agents/{id}/resume")),
            (Method::POST, format!("/api/agents/{id}/close")),
            (Method::POST, format!("/api/agents/{id}/archive")),
            (Method::POST, "/api/repos/app/notes".to_owned()),
            (Method::POST, "/api/notes".to_owned()),
            (Method::POST, "/api/orchestrator/reset".to_owned()),
            (Method::POST, "/api/messages".to_owned()),
            (Method::GET, "/ws".to_owned()),
            (Method::GET, format!("/ws/terminal/{id}")),
            // The token is read in the address of a socket only, and must be the right one.
            (Method::GET, format!("/api/state?token={TOKEN}")),
            (Method::GET, "/ws?token=guess".to_owned()),
        ];
        for (method, path) in guarded {
            let answer = factory.without_token(method, &path).await;
            assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{path}");
        }
        assert!(factory.env.agent(id).archived_at.is_none());
        assert_eq!(
            factory.without_token(Method::GET, "/api/health").await.status,
            StatusCode::OK
        );
        // No page is served: a browser opened on the daemon finds nothing.
        assert_eq!(factory.get("/").await.status, StatusCode::NOT_FOUND);
        assert_eq!(factory.get("/index.html").await.status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn lets_the_page_of_the_shell_call_it_from_its_own_origin() {
        let factory = factory();
        let send = |method: Method, has_token: bool| {
            let mut request = axum::http::Request::builder()
                .method(method.clone())
                .uri("/api/state")
                .header(HOST, HOST_HEADER)
                .header(ORIGIN, "tauri://localhost");
            if method == Method::OPTIONS {
                request = request
                    .header(ACCESS_CONTROL_REQUEST_METHOD, "GET")
                    .header("access-control-request-headers", "authorization");
            }
            if has_token {
                request = request.header(AUTHORIZATION, format!("Bearer {TOKEN}"));
            }
            let router = factory.router.clone();
            async move {
                let response = router
                    .oneshot(request.body(Body::empty()).expect("request"))
                    .await
                    .expect("response");
                (response.status(), response.headers().clone())
            }
        };
        // A webview asks first, without the token, whether it may send the header that carries it.
        let (status, headers) = send(Method::OPTIONS, false).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(headers[ACCESS_CONTROL_ALLOW_ORIGIN], "tauri://localhost");
        assert_eq!(headers[ACCESS_CONTROL_ALLOW_HEADERS], "authorization, content-type");
        let (status, headers) = send(Method::GET, true).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[ACCESS_CONTROL_ALLOW_ORIGIN], "tauri://localhost");
        // A refusal is readable by the page too.
        let (status, headers) = send(Method::GET, false).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(headers[ACCESS_CONTROL_ALLOW_ORIGIN], "tauri://localhost");
    }

    #[tokio::test]
    async fn validates_agent_creation_and_launches_the_agent() {
        let factory = factory();
        let invalid = factory
            .post("/api/agents", Some(json!({ "ticketRef": "", "steps": [] })))
            .await;
        assert_eq!(invalid.status, StatusCode::BAD_REQUEST);
        let issues = invalid.json();
        let paths: Vec<&str> = issues["error"]
            .as_array()
            .expect("issues")
            .iter()
            .filter_map(|issue| issue["path"].as_str())
            .collect();
        assert_eq!(paths, ["ticketRef", "steps"]);
        assert_eq!(factory.post("/api/agents", None).await.status, StatusCode::BAD_REQUEST);

        let unknown = factory
            .post("/api/agents", Some(json!({ "ticketRef": "NOPE-1" })))
            .await;
        assert_eq!(unknown.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            unknown.json()["error"]
                .as_str()
                .expect("error")
                .starts_with("aucun projet ne déclare le préfixe NOPE")
        );

        let created = factory
            .post(
                "/api/agents",
                Some(json!({ "ticketRef": "PROJ-9", "repoPath": factory.env.repo_path() })),
            )
            .await;
        assert_eq!(created.status, StatusCode::CREATED);
        assert_eq!(created.json()["ticketRef"], "PROJ-9");
        assert_eq!(factory.env.terminals.count(), 1);
    }

    #[tokio::test]
    async fn keeps_a_note_in_the_memory_of_a_project_when_the_master_accepts_it() {
        let factory = factory();
        let project = RepoInput {
            name: "shop".to_owned(),
            path: factory.env.repo_path(),
            ..RepoInput::default()
        };
        factory.env.service.projects().add(project).await.expect("project");
        let note = json!({ "note": "L'API tourne sur api.test" });
        assert_eq!(
            factory.post("/api/repos/shop/notes", Some(note.clone())).await.status,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            factory.env.service.projects().require("shop").expect("project").notes,
            ["L'API tourne sur api.test"]
        );
        assert_eq!(
            factory.post("/api/repos/ghost/notes", Some(note.clone())).await.status,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            factory
                .post("/api/repos/shop/notes", Some(json!({ "note": "a\nb" })))
                .await
                .status,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            factory.post("/api/notes", Some(note)).await.status,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            factory.env.service.projects().list_global_notes().expect("notes"),
            ["L'API tourne sur api.test"]
        );
    }

    #[tokio::test]
    async fn archives_an_agent_which_leaves_the_snapshot() {
        let factory = factory();
        let agent = factory.launched().await;
        assert_eq!(
            factory
                .post(&format!("/api/agents/{}/archive", agent.id), None)
                .await
                .status,
            StatusCode::NO_CONTENT
        );
        assert_eq!(factory.get("/api/state").await.json()["agents"], json!([]));
    }

    #[tokio::test]
    async fn resumes_and_closes_sessions_with_404_and_409_on_misuse() {
        let factory = factory();
        let agent = factory.launched().await;
        let path = |action: &str| format!("/api/agents/{}/{action}", agent.id);
        assert_eq!(
            factory.post("/api/agents/ghost/resume", None).await.status,
            StatusCode::NOT_FOUND
        );
        let busy = factory.post(&path("resume"), None).await;
        assert_eq!(
            (busy.status, busy.json()),
            (
                StatusCode::CONFLICT,
                json!({ "error": "l'agent PROJ-224 a déjà une session ouverte" })
            )
        );
        assert_eq!(factory.post(&path("close"), None).await.status, StatusCode::NO_CONTENT);
        assert_eq!(factory.post(&path("resume"), None).await.status, StatusCode::NO_CONTENT);
        assert_eq!(factory.env.terminals.count(), 2);

        let sessions = factory.get(&path("sessions")).await.json();
        assert_eq!(
            (&sessions[0]["agentId"], &sessions[0]["group"], &sessions[0]["stepId"]),
            (&json!(agent.id), &json!("cadrage"), &json!("qualification"))
        );
        assert_eq!(
            factory.get("/api/agents/ghost/sessions").await.status,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn sends_chat_messages_to_the_orchestrator_and_answers_broadcasts_without_it() {
        let factory = factory();
        let agent = factory.launched().await;
        let chat = json!({ "text": "  Où en est PROJ-224 ?  ", "mentions": [agent.id] });
        assert_eq!(
            factory.post("/api/messages", Some(chat)).await.status,
            StatusCode::ACCEPTED
        );
        settle().await;
        assert!(factory.prompts()[0].starts_with("Où en est PROJ-224 ?\n\nAgents mentioned:\n- PROJ-224"));
        assert_eq!(factory.env.last_message().text, "Agent lancé.");

        let broadcast = json!({ "text": "Ne poussez rien.", "broadcast": true });
        assert_eq!(
            factory.post("/api/messages", Some(broadcast)).await.status,
            StatusCode::ACCEPTED
        );
        settle().await;
        assert_eq!(factory.env.last_message().text, "Message diffusé à 1 agent.");
        assert_eq!(factory.prompts().len(), 1);
        assert_eq!(
            factory.post("/api/messages", Some(json!({ "text": " " }))).await.status,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn sends_to_the_orchestrator_a_broadcast_no_active_agent_would_receive() {
        let factory = factory();
        let broadcast = json!({ "text": "Oui, lance-le.", "broadcast": true });
        assert_eq!(
            factory.post("/api/messages", Some(broadcast)).await.status,
            StatusCode::ACCEPTED
        );
        settle().await;
        let texts: Vec<String> = factory.env.messages().into_iter().map(|message| message.text).collect();
        assert_eq!(
            texts,
            [
                "Oui, lance-le.",
                "Aucun agent actif à qui diffuser : le message part à l'orchestrateur.",
                "Agent lancé."
            ]
        );
        assert_eq!(factory.prompts(), ["Oui, lance-le."]);
    }

    #[tokio::test]
    async fn empties_the_thread_of_the_snapshot_on_a_reset() {
        let factory = factory();
        factory.post("/api/messages", Some(json!({ "text": "Bonjour" }))).await;
        settle().await;
        factory.env.clock.advance_minutes(1);
        assert_eq!(
            factory.post("/api/orchestrator/reset", None).await.status,
            StatusCode::NO_CONTENT
        );
        assert_eq!(factory.get("/api/state").await.json()["messages"], json!([]));
    }
}

mod daemon_lifecycle {
    use super::*;

    #[tokio::test]
    async fn names_itself_on_the_health_endpoint() {
        let answer = factory().get("/api/health").await;
        assert_eq!(answer.json(), json!({ "name": "atlas", "version": DAEMON_VERSION }));
    }

    #[tokio::test]
    async fn stops_only_for_a_client_that_holds_the_token() {
        let factory = factory();
        let stopped = Arc::clone(&factory.stop);
        let waiting = tokio::spawn(async move { stopped.notified().await });
        assert_eq!(
            factory.without_token(Method::POST, "/api/daemon/stop").await.status,
            StatusCode::UNAUTHORIZED
        );
        settle().await;
        assert!(!waiting.is_finished());
        assert_eq!(
            factory.post("/api/daemon/stop", None).await.status,
            StatusCode::ACCEPTED
        );
        waiting.await.expect("stop requested");
    }
}

mod hooks_endpoint {
    use super::*;

    #[tokio::test]
    async fn requires_the_token_and_a_known_agent() {
        let factory = factory();
        let agent = factory.launched().await;
        let hook = |event: &'static str, agent_id: Option<String>, has_token: bool| {
            let mut request = axum::http::Request::builder()
                .method(Method::POST)
                .uri(format!("/hooks/{event}"))
                .header(HOST, HOST_HEADER);
            if has_token {
                request = request.header(AUTHORIZATION, format!("Bearer {TOKEN}"));
            }
            if let Some(agent_id) = agent_id {
                request = request.header("x-atlas-agent", agent_id);
            }
            let router = factory.router.clone();
            async move {
                let response = router
                    .oneshot(request.body(Body::from("{\"prompt\":\"réponse\"}")).expect("request"))
                    .await
                    .expect("response");
                let status = response.status();
                let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("body");
                (status, serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null))
            }
        };
        assert_eq!(
            hook("stop", Some(agent.id.clone()), false).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            hook("stop", Some("ghost".to_owned()), true).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(hook("stop", None, true).await.0, StatusCode::BAD_REQUEST);
        assert_eq!(
            hook("unknown", Some(agent.id.clone()), true).await.0,
            StatusCode::BAD_REQUEST
        );

        let (status, stop) = hook("stop", Some(agent.id.clone()), true).await;
        assert_eq!((status, &stop["output"]["decision"]), (StatusCode::OK, &json!("block")));
        let (_, start) = hook("session-start", Some(agent.id.clone()), true).await;
        assert_eq!(start["output"]["hookSpecificOutput"]["hookEventName"], "SessionStart");
        assert!(
            start["output"]["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .expect("context")
                .contains("PROJ-224")
        );
        assert_eq!(
            hook("post-tool-use", Some(agent.id.clone()), true).await.1,
            json!({ "output": null })
        );
        assert_eq!(
            hook("notification", Some(agent.id.clone()), true).await.1,
            json!({ "output": null })
        );

        factory.env.service.ask_user(&agent.id, "Question").expect("ask");
        assert_eq!(
            hook("user-prompt-submit", Some(agent.id.clone()), true).await.1,
            json!({ "output": null })
        );
        assert_eq!(factory.env.agent(&agent.id).status, AgentStatus::Working);
    }
}

mod mcp_servers {
    use super::*;

    #[tokio::test]
    async fn serves_the_workflow_tools_to_the_agent_bound_by_url() {
        let factory = factory();
        let agent = factory.launched().await;
        let path = format!("/mcp/agent/{}", agent.id);
        let initialized =
            factory.rpc(&path, "initialize", json!({ "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": { "name": "claude", "version": "2" } })).await;
        assert_eq!(initialized["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(
            initialized["result"]["serverInfo"],
            json!({ "name": "atlas", "version": DAEMON_VERSION })
        );
        assert_eq!(
            initialized["result"]["capabilities"],
            json!({ "tools": { "listChanged": true } })
        );

        let listed = factory.rpc(&path, "tools/list", json!({})).await;
        let names: Vec<&str> = listed["result"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|tool| tool["name"].as_str().expect("name"))
            .collect();
        assert_eq!(
            names,
            [
                "next_step",
                "complete_step",
                "work_on_branch",
                "request_repo",
                "propose_note",
                "ask_user",
                "block"
            ]
        );

        let (brief, is_error) = factory.call_tool(&path, "next_step", json!({})).await;
        assert!(brief.starts_with("Step: Qualification") && !is_error);
        let (refused, is_error) = factory
            .call_tool(&path, "complete_step", json!({ "fields": { "need": "x" } }))
            .await;
        assert!(refused.starts_with("champs manquants dans fields : ticket") && is_error);
        let (invalid, is_error) = factory.call_tool(&path, "block", json!({})).await;
        assert!(invalid.starts_with("Invalid arguments for tool block") && is_error);
        let (unknown, is_error) = factory.call_tool(&path, "launch_agent", json!({})).await;
        assert_eq!(
            (unknown.as_str(), is_error),
            ("MCP error -32602: Tool launch_agent not found", true)
        );

        let (asked, _) = factory
            .call_tool(&path, "ask_user", json!({ "question": "Quel délai ?" }))
            .await;
        assert!(asked.starts_with("The master is told"));
        assert_eq!(factory.env.agent(&agent.id).status, AgentStatus::Waiting);
    }

    #[tokio::test]
    async fn speaks_json_rpc_notifications_batches_and_unknown_methods() {
        let factory = factory();
        let path = "/mcp/orchestrator";
        let notification = factory
            .post(
                path,
                Some(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })),
            )
            .await;
        assert_eq!(
            (notification.status, notification.body.as_str()),
            (StatusCode::ACCEPTED, "")
        );
        assert_eq!(
            factory.rpc(path, "ping", json!({})).await,
            json!({ "jsonrpc": "2.0", "id": 1, "result": {} })
        );
        let unknown = factory.rpc(path, "resources/list", json!({})).await;
        assert_eq!(unknown["error"]["code"], -32601);

        let batch = json!([
            { "jsonrpc": "2.0", "id": "a", "method": "ping" },
            { "jsonrpc": "2.0", "method": "notifications/initialized" },
            { "jsonrpc": "2.0", "id": "b", "method": "tools/list" }
        ]);
        let answers = factory.post(path, Some(batch)).await.json();
        let ids: Vec<&str> = answers
            .as_array()
            .expect("batch")
            .iter()
            .map(|answer| answer["id"].as_str().expect("id"))
            .collect();
        assert_eq!(ids, ["a", "b"]);

        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri(path)
            .header(HOST, HOST_HEADER);
        let request = request
            .header(AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::from("not json"))
            .expect("request");
        assert_eq!(
            factory
                .router
                .clone()
                .oneshot(request)
                .await
                .expect("response")
                .status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn keeps_other_local_programs_out() {
        let factory = factory();
        let agent = factory.launched().await;
        let ping = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });
        assert_eq!(
            factory.without_token(Method::POST, "/mcp/orchestrator").await.status,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            factory
                .without_token(Method::POST, &format!("/mcp/agent/{}", agent.id))
                .await
                .status,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            factory.post("/mcp/agent/ghost", Some(ping)).await.status,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            factory.get("/mcp/orchestrator").await.status,
            StatusCode::METHOD_NOT_ALLOWED
        );
        let delete = factory
            .request(
                Method::DELETE,
                &format!("/mcp/agent/{}", agent.id),
                HOST_HEADER,
                true,
                None,
            )
            .await;
        assert_eq!(delete.status, StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn lets_the_orchestrator_compose_follow_and_change_a_workflow_in_a_few_short_lines() {
        let factory = factory();
        let path = "/mcp/orchestrator";
        let (none, _) = factory.call_tool(path, "list_agents", json!({})).await;
        assert_eq!(none, "No agent.");

        let project = json!({ "name": "shop", "path": factory.env.repo_path(), "ticketPrefixes": ["proj"] });
        let (added, is_error) = factory.call_tool(path, "add_repo", project).await;
        assert!(!is_error, "{added}");
        assert_eq!(
            serde_json::from_str::<Value>(&added).expect("json")["ticketPrefixes"],
            json!(["PROJ"])
        );

        let launch = json!({ "ticketRef": "PROJ-7", "title": "Corriger l'export", "steps": ["qualification", "implementation", "tests"], "reason": "Petit correctif." });
        let (launched, is_error) = factory.call_tool(path, "launch_agent", launch).await;
        assert!(!is_error, "{launched}");
        let agent = factory.env.store.list_agents(false).expect("agents").remove(0);
        assert_eq!(
            launched,
            format!(
                "Launched. PROJ-7 | Corriger l'export | project shop | working | session open | [qualification] > implementation > tests | agentId {}",
                agent.id
            )
        );
        assert_eq!(factory.env.last_message().text, "Petit correctif.");
        let (incomplete, is_error) = factory
            .call_tool(
                path,
                "launch_agent",
                json!({ "ticketRef": "PROJ-8", "steps": ["tache"] }),
            )
            .await;
        assert_eq!(
            (incomplete.as_str(), is_error),
            ("reason : la raison du parcours est requise", true)
        );

        let (detail, _) = factory
            .call_tool(path, "get_agent", json!({ "agentId": agent.id }))
            .await;
        let detail: Value = serde_json::from_str(&detail).expect("json");
        assert_eq!(detail["sessions"].as_array().expect("sessions").len(), 1);
        assert!(detail.get("report").is_none() && detail.get("note").is_some());

        let change = json!({ "agentId": agent.id, "steps": ["implementation", "tests", "review", "ship"], "reason": "Le maître veut une MR." });
        let (changed, _) = factory.call_tool(path, "set_workflow", change).await;
        assert!(
            changed.starts_with("Workflow changed. PROJ-7")
                && changed.contains("[qualification] > implementation > tests > review > ship")
        );

        let (screen, _) = factory
            .call_tool(path, "read_agent_terminal", json!({ "agentId": agent.id }))
            .await;
        assert!(
            screen.starts_with("Launch command:\nclaude --session-id")
                && screen.ends_with("End of the screen:\n(empty screen)")
        );
        let (stopped, _) = factory
            .call_tool(path, "stop_agent", json!({ "agentId": agent.id, "reason": "pause" }))
            .await;
        assert!(
            stopped.starts_with("Agent stopped. PROJ-7")
                && stopped.contains("blocked (arrêté : pause) | session closed")
        );
        let (resumed, _) = factory
            .call_tool(path, "resume_agent", json!({ "agentId": agent.id }))
            .await;
        assert!(resumed.starts_with("Session reopened. PROJ-7"));
        let (ghost, is_error) = factory
            .call_tool(path, "archive_agent", json!({ "agentId": "ghost" }))
            .await;
        assert_eq!((ghost.as_str(), is_error), ("agent inconnu : ghost", true));
        let (told, _) = factory
            .call_tool(path, "broadcast", json!({ "text": "Pause à midi." }))
            .await;
        assert_eq!(told, "Message broadcast to 1 agent(s).");
        let (repos, _) = factory.call_tool(path, "list_repos", json!({})).await;
        assert_eq!(
            serde_json::from_str::<Value>(&repos).expect("json")["repos"][0]["name"],
            "shop"
        );
    }

    #[tokio::test]
    async fn lets_the_orchestrator_decide_on_a_step_waiting_for_it() {
        let factory = factory();
        let mut wakeups = factory.env.wakeups();
        let agent = factory.launched().await;
        let (service, agent_id, input) = (
            factory.env.service.clone(),
            agent.id.clone(),
            factory.env.fields_for("qualification", &[]),
        );
        let closing = tokio::spawn(async move { service.complete_step(&agent_id, input).await });
        wakeups.recv().await.expect("approval request");
        let decision =
            json!({ "agentId": agent.id, "reason": "Plan gardé.", "steps": ids(&["plan", "implementation"]) });
        let (decided, is_error) = factory.call_tool("/mcp/orchestrator", "approve_step", decision).await;
        assert!(
            decided.starts_with("Decision recorded. PROJ-224") && !is_error,
            "{decided}"
        );
        closing.await.expect("join").expect("reply");
        assert_eq!(
            factory.env.path(&agent.id),
            [
                "qualification:done",
                "tests:skipped",
                "review:skipped",
                "correction:skipped",
                "ship:skipped",
                "plan:current",
                "implementation:upcoming"
            ]
        );
    }
}

mod websockets {
    use super::*;

    type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

    /// Serves the factory on a free port, as the daemon does.
    async fn served(factory: &Factory) -> u16 {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let port = listener.local_addr().expect("address").port();
        let router = factory.router.clone();
        tokio::spawn(async move { axum::serve(listener, router).await });
        port
    }

    async fn next_event(socket: &mut Socket) -> Value {
        loop {
            match socket.next().await.expect("an event").expect("a frame") {
                WsMessage::Text(text) => return serde_json::from_str(text.as_str()).expect("json"),
                WsMessage::Close(_) => panic!("socket closed"),
                _ => {}
            }
        }
    }

    async fn send(socket: &mut Socket, event: Value) {
        socket
            .send(WsMessage::Text(event.to_string().into()))
            .await
            .expect("send");
    }

    #[tokio::test]
    async fn pushes_the_snapshot_then_what_happens_in_the_factory() {
        let factory = factory();
        let agent = factory.launched().await;
        let port = served(&factory).await;
        let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws?token={TOKEN}"))
            .await
            .expect("connect");
        let snapshot = next_event(&mut socket).await;
        assert_eq!(
            (&snapshot["type"], &snapshot["snapshot"]["agents"][0]["id"]),
            (&json!("snapshot"), &json!(agent.id))
        );

        factory.env.service.ask_user(&agent.id, "Question").expect("ask");
        let updated = next_event(&mut socket).await;
        assert_eq!(
            (&updated["type"], &updated["agent"]["status"]),
            (&json!("agent"), &json!("waiting"))
        );
        assert_eq!(
            next_event(&mut socket).await["message"]["text"],
            "L'agent qui travaille sur PROJ-224 a besoin de votre aide."
        );

        factory.env.service.archive(&agent.id).expect("archive");
        let closed = next_event(&mut socket).await;
        assert_eq!(
            (&closed["type"], &closed["agent"]["liveSessionId"]),
            (&json!("agent"), &Value::Null)
        );
        // The last update of an archived agent must not bring it back on the board: only its leaving is told.
        assert_eq!(
            next_event(&mut socket).await,
            json!({ "type": "archived", "agentId": agent.id })
        );

        // A change of language is told with the whole state, which carries it.
        factory
            .env
            .service
            .set_language(crate::contract::Language::En)
            .expect("language");
        let spoken = next_event(&mut socket).await;
        assert_eq!(
            (&spoken["type"], &spoken["snapshot"]["language"]),
            (&json!("snapshot"), &json!("en"))
        );
    }

    #[tokio::test]
    async fn refuses_a_socket_opened_from_another_website() {
        let factory = factory();
        let agent = factory.launched().await;
        let port = served(&factory).await;
        let mut request = format!("ws://127.0.0.1:{port}/ws?token={TOKEN}")
            .into_client_request()
            .expect("request");
        request
            .headers_mut()
            .insert("Origin", "https://evil.example".parse().expect("origin"));
        assert!(tokio_tungstenite::connect_async(request).await.is_err());
        // The terminal of an agent has no other guard: typing into it is driving the agent.
        let mut terminal = format!("ws://127.0.0.1:{port}/ws/terminal/{}?token={TOKEN}", agent.id)
            .into_client_request()
            .expect("request");
        terminal
            .headers_mut()
            .insert("Origin", "https://evil.example".parse().expect("origin"));
        assert!(tokio_tungstenite::connect_async(terminal).await.is_err());
        let mut local = format!("ws://127.0.0.1:{port}/ws?token={TOKEN}")
            .into_client_request()
            .expect("request");
        local
            .headers_mut()
            .insert("Origin", "tauri://localhost".parse().expect("origin"));
        assert!(tokio_tungstenite::connect_async(local).await.is_ok());
    }

    #[tokio::test]
    async fn redraws_the_terminal_at_the_size_of_the_browser_then_follows_the_session() {
        let factory = factory();
        let agent = factory.launched().await;
        let session = factory.env.terminals.last();
        session.print("déjà affiché\r\n");
        let port = served(&factory).await;
        let url = format!("ws://127.0.0.1:{port}/ws/terminal/{}?token={TOKEN}", agent.id);
        let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.expect("connect");

        // Nothing is sent before the browser gives its size.
        session.print("avant la taille\r\n");
        send(&mut socket, json!({ "type": "resize", "cols": 100, "rows": 30 })).await;
        let screen = next_event(&mut socket).await;
        let drawn = screen["data"].as_str().expect("screen");
        assert_eq!(screen["type"], "output");
        assert!(drawn.contains("déjà affiché") && drawn.contains("avant la taille"));

        session.print("ensuite");
        assert_eq!(
            next_event(&mut socket).await,
            json!({ "type": "output", "data": "ensuite" })
        );
        send(&mut socket, json!({ "type": "input", "data": "oui\r" })).await;
        send(&mut socket, json!({ "type": "nonsense" })).await;
        settle().await;
        assert_eq!(session.written(), "oui\r");

        session.exit(Some(0));
        assert_eq!(next_event(&mut socket).await, json!({ "type": "exit", "code": 0 }));
    }

    #[tokio::test]
    async fn tells_at_once_that_an_agent_has_no_open_session() {
        let factory = factory();
        let agent = factory.launched().await;
        factory.env.service.close(&agent.id).expect("close");
        let port = served(&factory).await;
        let url = format!("ws://127.0.0.1:{port}/ws/terminal/{}?token={TOKEN}", agent.id);
        let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.expect("connect");
        assert_eq!(next_event(&mut socket).await, json!({ "type": "exit", "code": null }));
    }
}

#[test]
fn reads_the_host_of_a_header_with_or_without_a_port() {
    assert_eq!(hostname("127.0.0.1:4317"), "127.0.0.1");
    assert_eq!(hostname("[::1]:4317"), "[::1]");
    assert_eq!(hostname("localhost"), "localhost");
}

#[test]
fn launches_from_the_api_with_the_same_input_as_the_orchestrator() {
    let input: CreateAgentInput =
        serde_json::from_value(json!({ "ticketRef": "PROJ-1", "inPlace": true, "branchType": "fix" })).expect("input");
    assert_eq!(
        (input.in_place, input.branch_type.as_deref()),
        (Some(true), Some("fix"))
    );
}
