// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The HTTP API and the WebSockets of the daemon, on the loopback interface only.
//!
//! The daemon serves no page: the front ships inside the desktop shell, which holds the token every
//! route but the health check asks for.

use crate::language::tr;
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Request, State};
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    ACCESS_CONTROL_REQUEST_METHOD, AUTHORIZATION, HOST, ORIGIN, VARY,
};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::sync::Notify;
use tokio::sync::broadcast::error::RecvError;

use crate::contract::{
    BrickSummary, CatalogueDecisionInput, CreateAgentInput, DAEMON_NAME, DAEMON_VERSION, DaemonHealth, Issue,
    LanguageInput, MessageRole, PostMessageInput, ServerEvent, Snapshot, TerminalClientEvent, TerminalServerEvent,
    parse_repo_note,
};
use crate::error::{Error, Result};
use crate::mcp::McpServer;
use crate::orchestrator::Orchestrator;
use crate::service::{AgentService, Event, TerminalEvent};
use crate::store::{MessageExtras, Store, to_agent_view};

pub struct ServerDeps {
    pub store: Store,
    pub service: AgentService,
    pub orchestrator: Orchestrator,
    /// Shared secret the desktop shell, the agents' MCP client and the hooks present; keeps other
    /// local programs and any browser out.
    pub token: String,
    /// Told when a client that owns the token asks the daemon to stop, such as the desktop shell.
    pub stop: Arc<Notify>,
}

type AppState = Arc<ServerDeps>;

const LOCAL_HOSTS: [&str; 3] = ["127.0.0.1", "localhost", "[::1]"];
/// Hosts of the pages allowed to call the daemon: the desktop shell (`tauri://localhost`, or
/// `http://tauri.localhost` on Windows) and the Vite server it loads in development.
const PAGE_HOSTS: [&str; 4] = ["127.0.0.1", "localhost", "[::1]", "tauri.localhost"];
const SOCKET_PREFIX: &str = "/ws";
const TOKEN_PARAMETER: &str = "token";
const ALLOWED_HEADERS: HeaderValue = HeaderValue::from_static("authorization, content-type");
const ALLOWED_METHODS: HeaderValue = HeaderValue::from_static("GET, POST");
/// Resets the terminal of the page before its screen is drawn again from scratch.
const TERMINAL_RESET: &str = "\u{1b}c";

fn hostname(authority: &str) -> &str {
    match authority.find(']') {
        Some(end) if authority.starts_with('[') => &authority[..=end],
        _ => authority.split(':').next().unwrap_or_default(),
    }
}

fn is_local_host(headers: &HeaderMap) -> bool {
    headers
        .get(HOST)
        .and_then(|host| host.to_str().ok())
        .is_some_and(|host| LOCAL_HOSTS.contains(&hostname(host)))
}

/// Whether the request comes from the page of the desktop shell, or from no page at all (a local program).
fn is_shell_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(ORIGIN) else {
        return true;
    };
    origin
        .to_str()
        .ok()
        .and_then(|origin| origin.split_once("://"))
        .is_some_and(|(_, authority)| PAGE_HOSTS.contains(&hostname(authority.split('/').next().unwrap_or_default())))
}

fn holds_token(request: &Request, token: &str) -> bool {
    let headers = request.headers();
    if headers.get(AUTHORIZATION).and_then(|value| value.to_str().ok()) == Some(format!("Bearer {token}").as_str()) {
        return true;
    }
    // A page cannot set a header on a WebSocket: the shell gives the token in the address instead.
    let uri = request.uri();
    uri.path().starts_with(SOCKET_PREFIX)
        && uri.query().is_some_and(|query| {
            query
                .split('&')
                .any(|pair| pair.split_once('=') == Some((TOKEN_PARAMETER, token)))
        })
}

fn error_response(status: StatusCode, error: impl serde::Serialize) -> Response {
    (status, axum::Json(json!({ "error": error }))).into_response()
}

fn missing_token() -> Response {
    error_response(StatusCode::UNAUTHORIZED, &tr!("jeton manquant", "missing token"))
}

fn unknown_agent() -> Response {
    error_response(StatusCode::NOT_FOUND, &tr!("agent inconnu", "unknown agent"))
}

/// Answers an expected refusal with `status` and its message; anything else is a bug and answers 500.
fn failure(error: Error, status: StatusCode) -> Response {
    match error {
        Error::Refusal(message) => error_response(status, message),
        Error::Internal(message) => {
            tracing::error!("{message}");
            error_response(StatusCode::INTERNAL_SERVER_ERROR, message)
        }
    }
}

fn internal(error: Error) -> Response {
    failure(error, StatusCode::INTERNAL_SERVER_ERROR)
}

/// The JSON body of a request as `T`, checked by `validate`; the issues answer 400.
fn parse_body<T: DeserializeOwned>(
    body: &Bytes,
    validate: impl FnOnce(T) -> std::result::Result<T, Vec<Issue>>,
) -> std::result::Result<T, Box<Response>> {
    let parsed: T = serde_json::from_slice(body).map_err(|error| {
        Box::new(error_response(
            StatusCode::BAD_REQUEST,
            vec![Issue::new("", error.to_string())],
        ))
    })?;
    validate(parsed).map_err(|issues| Box::new(error_response(StatusCode::BAD_REQUEST, issues)))
}

/// Refuses what does not come from this machine: a request that does not target localhost (DNS
/// rebinding), and one sent by a page of another website, which a browser lets through to
/// 127.0.0.1 without asking (a form or a `fetch` with a plain body). The page of the shell is of
/// another origin than the daemon, so its requests are answered with the headers a webview asks for.
async fn local_only(request: Request, next: Next) -> Response {
    if !is_local_host(request.headers()) {
        return error_response(StatusCode::FORBIDDEN, &tr!("hôte refusé", "host refused"));
    }
    if !is_shell_origin(request.headers()) {
        return error_response(StatusCode::FORBIDDEN, &tr!("origine refusée", "origin refused"));
    }
    let origin = request.headers().get(ORIGIN).cloned();
    let is_preflight =
        request.method() == Method::OPTIONS && request.headers().contains_key(ACCESS_CONTROL_REQUEST_METHOD);
    let mut response = if is_preflight {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(request).await
    };
    if let Some(origin) = origin {
        let headers = response.headers_mut();
        headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        headers.insert(ACCESS_CONTROL_ALLOW_HEADERS, ALLOWED_HEADERS);
        headers.insert(ACCESS_CONTROL_ALLOW_METHODS, ALLOWED_METHODS);
        headers.insert(VARY, HeaderValue::from_static("origin"));
    }
    response
}

/// Refuses a client that does not hold the shared secret of this machine.
async fn token_only(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if !holds_token(&request, &state.token) {
        return missing_token();
    }
    next.run(request).await
}

fn snapshot(state: &ServerDeps) -> Result<Snapshot> {
    Ok(Snapshot {
        language: crate::language::current(),
        bricks: state
            .service
            .catalogue()
            .values()
            .map(|brick| BrickSummary {
                id: brick.id.clone(),
                name: brick.name.clone(),
            })
            .collect(),
        agents: state.service.views()?,
        messages: state.orchestrator.conversation()?,
        orchestrator_busy: state.orchestrator.is_busy(),
    })
}

async fn get_state(State(state): State<AppState>) -> Response {
    snapshot(&state).map_or_else(internal, |snapshot| axum::Json(snapshot).into_response())
}

async fn get_health() -> axum::Json<DaemonHealth> {
    axum::Json(DaemonHealth {
        name: DAEMON_NAME,
        version: DAEMON_VERSION,
    })
}

async fn stop_daemon(State(state): State<AppState>) -> Response {
    state.stop.notify_one();
    StatusCode::ACCEPTED.into_response()
}

async fn list_sessions(State(state): State<AppState>, Path(agent_id): Path<String>) -> Response {
    match state.store.get_agent(&agent_id) {
        Ok(None) => unknown_agent(),
        Ok(Some(_)) => state
            .store
            .list_sessions(&agent_id)
            .map_or_else(internal, |sessions| axum::Json(sessions).into_response()),
        Err(error) => internal(error),
    }
}

async fn create_agent(State(state): State<AppState>, body: Bytes) -> Response {
    let input = match parse_body(&body, CreateAgentInput::validated) {
        Ok(input) => input,
        Err(response) => return *response,
    };
    match state.service.launch(input).await {
        Ok(agent) => (StatusCode::CREATED, axum::Json(to_agent_view(&agent))).into_response(),
        Err(error) => failure(error, StatusCode::UNPROCESSABLE_ENTITY),
    }
}

/// Runs an action on a known agent: 204 when done, 409 when Atlas refuses it.
fn agent_action(state: &ServerDeps, agent_id: &str, action: impl FnOnce(&AgentService) -> Result<()>) -> Response {
    match state.store.get_agent(agent_id) {
        Ok(None) => unknown_agent(),
        Ok(Some(_)) => match action(&state.service) {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(error) => failure(error, StatusCode::CONFLICT),
        },
        Err(error) => internal(error),
    }
}

async fn resume_agent(State(state): State<AppState>, Path(agent_id): Path<String>) -> Response {
    agent_action(&state, &agent_id, |service| service.resume(&agent_id, None).map(drop))
}

async fn close_agent(State(state): State<AppState>, Path(agent_id): Path<String>) -> Response {
    agent_action(&state, &agent_id, |service| service.close(&agent_id))
}

async fn archive_agent(State(state): State<AppState>, Path(agent_id): Path<String>) -> Response {
    agent_action(&state, &agent_id, |service| service.archive(&agent_id))
}

#[derive(Deserialize)]
struct NoteBody {
    note: String,
}

fn validated_note(body: NoteBody) -> std::result::Result<NoteBody, Vec<Issue>> {
    parse_repo_note(&body.note)
        .map(|note| NoteBody { note })
        .map_err(|reason| vec![Issue::new("note", reason)])
}

async fn add_repo_note(State(state): State<AppState>, Path(name): Path<String>, body: Bytes) -> Response {
    let input = match parse_body(&body, validated_note) {
        Ok(input) => input,
        Err(response) => return *response,
    };
    match state.service.projects().add_note(&name, &input.note) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => failure(error, StatusCode::UNPROCESSABLE_ENTITY),
    }
}

/// The master applies or refuses the change to the catalogue a message of the orchestrator proposes.
async fn decide_catalogue_change(State(state): State<AppState>, Path(id): Path<String>, body: Bytes) -> Response {
    let input: CatalogueDecisionInput = match parse_body(&body, Ok) {
        Ok(input) => input,
        Err(response) => return *response,
    };
    match state.service.decide_catalogue_change(&id, input.decision) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => failure(error, StatusCode::UNPROCESSABLE_ENTITY),
    }
}

async fn add_global_note(State(state): State<AppState>, body: Bytes) -> Response {
    let input = match parse_body(&body, validated_note) {
        Ok(input) => input,
        Err(response) => return *response,
    };
    match state.service.projects().add_global_note(&input.note) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => internal(error),
    }
}

/// The master picks the language of the factory; every client is told with a new snapshot.
async fn set_language(State(state): State<AppState>, body: Bytes) -> Response {
    let input: LanguageInput = match parse_body(&body, Ok) {
        Ok(input) => input,
        Err(response) => return *response,
    };
    match state.service.set_language(input.language) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => failure(error, StatusCode::UNPROCESSABLE_ENTITY),
    }
}

async fn reset_orchestrator(State(state): State<AppState>) -> Response {
    match state.orchestrator.reset() {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => failure(error, StatusCode::CONFLICT),
    }
}

/// Stores a chat message and tells the clients.
fn say(state: &ServerDeps, role: MessageRole, text: &str) -> Result<()> {
    let message = state.store.add_message(role, text, None, MessageExtras::default())?;
    state.service.emit(Event::Message(Box::new(message)));
    Ok(())
}

fn post_message_to_factory(state: &ServerDeps, input: PostMessageInput) -> Result<()> {
    say(state, MessageRole::User, &input.text)?;
    if input.broadcast && state.service.active_agent_count()? > 0 {
        let count = state.service.broadcast(&input.text)?;
        return say(
            state,
            MessageRole::Assistant,
            &tr!(
                "Message diffusé à {count} agent{}.",
                "Message broadcast to {count} agent{}.",
                if count > 1 { "s" } else { "" }
            ),
        );
    }
    // A broadcast nobody would receive is most often an answer to the orchestrator sent with the toggle left on.
    if input.broadcast {
        say(
            state,
            MessageRole::Assistant,
            &tr!(
                "Aucun agent actif à qui diffuser : le message part à l'orchestrateur.",
                "No active agent to broadcast to: the message goes to the orchestrator."
            ),
        )?;
    }
    tokio::spawn(state.orchestrator.chat(input.text, input.mentions));
    Ok(())
}

async fn post_message(State(state): State<AppState>, body: Bytes) -> Response {
    let input = match parse_body(&body, PostMessageInput::validated) {
        Ok(input) => input,
        Err(response) => return *response,
    };
    post_message_to_factory(&state, input).map_or_else(internal, |()| StatusCode::ACCEPTED.into_response())
}

/// Output of a hook, printed verbatim on stdout by the hook command.
pub fn hook_output(service: &AgentService, event: &str, agent_id: &str, payload: &Value) -> Result<Option<Value>> {
    let context = |event_name: &str, context: String| json!({ "hookSpecificOutput": { "hookEventName": event_name, "additionalContext": context } });
    Ok(match event {
        "stop" => service
            .on_stop(agent_id)?
            .map(|reason| json!({ "decision": "block", "reason": reason })),
        "notification" => {
            service.on_idle(agent_id)?;
            None
        }
        "post-tool-use" => service
            .on_post_tool_use(agent_id)?
            .map(|broadcasts| context("PostToolUse", broadcasts)),
        "session-start" => Some(context("SessionStart", service.on_session_start(agent_id)?)),
        "user-prompt-submit" => {
            service.on_user_prompt(agent_id, payload.get("prompt").and_then(Value::as_str))?;
            None
        }
        _ => return Err(Error::refusal(&tr!("hook invalide", "invalid hook"))),
    })
}

async fn run_hook(
    State(state): State<AppState>,
    Path(event): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let agent_id = headers
        .get("x-atlas-agent")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let is_known = matches!(state.store.get_agent(agent_id), Ok(Some(_)));
    if !is_known || !crate::claude::HOOK_EVENTS.contains(&event.as_str()) {
        return error_response(StatusCode::BAD_REQUEST, &tr!("hook invalide", "invalid hook"));
    }
    let payload: Value = serde_json::from_slice(&body)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    match hook_output(&state.service, &event, agent_id, &payload) {
        Ok(output) => axum::Json(json!({ "output": output })).into_response(),
        Err(error) => failure(error, StatusCode::BAD_REQUEST),
    }
}

async fn serve_mcp(server: McpServer, body: &Bytes) -> Response {
    let Ok(message) = serde_json::from_slice::<Value>(body) else {
        let parse_error =
            json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "Parse error" } });
        return (StatusCode::BAD_REQUEST, axum::Json(parse_error)).into_response();
    };
    match server.handle(message).await {
        Some(answer) => axum::Json(answer).into_response(),
        // Only notifications: nothing to answer.
        None => StatusCode::ACCEPTED.into_response(),
    }
}

async fn agent_mcp(State(state): State<AppState>, Path(agent_id): Path<String>, body: Bytes) -> Response {
    match state.store.get_agent(&agent_id) {
        Ok(None) => unknown_agent(),
        Ok(Some(_)) => {
            serve_mcp(
                McpServer::Agent {
                    service: state.service.clone(),
                    agent_id,
                },
                &body,
            )
            .await
        }
        Err(error) => internal(error),
    }
}

async fn orchestrator_mcp(State(state): State<AppState>, body: Bytes) -> Response {
    serve_mcp(
        McpServer::Orchestrator {
            service: state.service.clone(),
            store: state.store.clone(),
        },
        &body,
    )
    .await
}

/// Stateless Streamable HTTP: no stream to open with GET, no session to end with DELETE.
async fn mcp_method_not_allowed() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

async fn send_json(socket: &mut WebSocket, event: &impl serde::Serialize) -> bool {
    match serde_json::to_string(event) {
        Ok(text) => socket.send(Message::Text(text.into())).await.is_ok(),
        Err(_) => false,
    }
}

/// What a client is sent for an event; none for one it does not hear of as such.
fn to_server_event(event: Event) -> Option<ServerEvent> {
    Some(match event {
        // Told with a whole snapshot, which carries the language.
        Event::Language => return None,
        // An archived agent left the board: its last update must not bring it back.
        Event::Agent(agent) if agent.archived_at.is_some() => return None,
        Event::Agent(agent) => ServerEvent::Agent {
            agent: Box::new(to_agent_view(&agent)),
        },
        Event::Archived(agent_id) => ServerEvent::Archived { agent_id },
        Event::Message(message) => ServerEvent::Message { message },
        Event::Busy(busy) => ServerEvent::Orchestrator { busy },
        Event::Reset => ServerEvent::Reset,
    })
}

async fn send_snapshot(socket: &mut WebSocket, state: &ServerDeps) -> bool {
    match snapshot(state) {
        Ok(snapshot) => {
            send_json(
                socket,
                &ServerEvent::Snapshot {
                    snapshot: Box::new(snapshot),
                },
            )
            .await
        }
        Err(error) => {
            tracing::error!(
                "{}",
                tr!(
                    "état de l'usine illisible : {error}",
                    "state of the factory unreadable: {error}"
                )
            );
            false
        }
    }
}

async fn factory_socket(mut socket: WebSocket, state: AppState) {
    let mut events = state.service.subscribe();
    if !send_snapshot(&mut socket, &state).await {
        return;
    }
    loop {
        tokio::select! {
            event = events.recv() => {
                let is_sent = match event {
                    Ok(Event::Language) => send_snapshot(&mut socket, &state).await,
                    Ok(event) => match to_server_event(event) {
                        Some(event) => send_json(&mut socket, &event).await,
                        None => true,
                    },
                    // Too slow to follow: the whole state replaces what was missed.
                    Err(RecvError::Lagged(_)) => send_snapshot(&mut socket, &state).await,
                    Err(RecvError::Closed) => false,
                };
                if !is_sent {
                    return;
                }
            }
            received = socket.recv() => {
                if !matches!(received, Some(Ok(_))) {
                    return;
                }
            }
        }
    }
}

async fn open_factory_socket(State(state): State<AppState>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| factory_socket(socket, state))
}

async fn terminal_socket(mut socket: WebSocket, state: AppState, agent_id: String) {
    let service = &state.service;
    if !service.has_live_session(&agent_id) {
        send_json(&mut socket, &TerminalServerEvent::Exit { code: None }).await;
        return;
    }
    // The client gives its size first: the session's terminal takes it, then the screen is sent as it
    // stands at that size, followed by what the session prints from then on.
    let mut output: Option<tokio::sync::broadcast::Receiver<TerminalEvent>> = None;
    let mut exits = service.subscribe_terminal();
    loop {
        tokio::select! {
            received = socket.recv() => {
                let Some(Ok(message)) = received else {
                    return;
                };
                let Message::Text(text) = message else {
                    continue;
                };
                match serde_json::from_str::<TerminalClientEvent>(text.as_str()) {
                    Ok(TerminalClientEvent::Input { data }) => service.write_to_terminal(&agent_id, &data),
                    Ok(TerminalClientEvent::Resize { cols, rows }) if cols > 0 && rows > 0 => {
                        service.resize_terminal(&agent_id, cols, rows);
                        if output.is_none() && let Some((screen, receiver)) = service.attach_terminal(&agent_id) {
                            if !screen.is_empty() && !send_json(&mut socket, &TerminalServerEvent::Output { data: screen }).await {
                                return;
                            }
                            output = Some(receiver);
                        }
                    }
                    _ => {}
                }
            }
            event = async {
                match output.as_mut() {
                    Some(output) => output.recv().await,
                    None => exits.recv().await,
                }
            } => {
                let is_attached = output.is_some();
                let to_send = match event {
                    Ok(TerminalEvent::Output { agent_id: source, data }) if source == agent_id && is_attached => {
                        Some(TerminalServerEvent::Output { data })
                    }
                    Ok(TerminalEvent::Exit { agent_id: source, code }) if source == agent_id => Some(TerminalServerEvent::Exit { code }),
                    Ok(_) => None,
                    // Too slow to follow: the screen is drawn again as it stands.
                    Err(RecvError::Lagged(_)) => match service.attach_terminal(&agent_id).filter(|_| is_attached) {
                        Some((screen, receiver)) => {
                            output = Some(receiver);
                            Some(TerminalServerEvent::Output { data: format!("{TERMINAL_RESET}{screen}") })
                        }
                        None => None,
                    },
                    Err(RecvError::Closed) => return,
                };
                if let Some(event) = to_send && !send_json(&mut socket, &event).await {
                    return;
                }
            }
        }
    }
}

async fn open_terminal_socket(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    upgrade: WebSocketUpgrade,
) -> Response {
    upgrade.on_upgrade(move |socket| terminal_socket(socket, state, agent_id))
}

pub fn build_router(deps: ServerDeps) -> Router {
    let state = Arc::new(deps);
    let protected = Router::new()
        .route("/api/state", get(get_state))
        .route("/api/daemon/stop", post(stop_daemon))
        .route("/api/agents", post(create_agent))
        .route("/api/agents/{id}/sessions", get(list_sessions))
        .route("/api/agents/{id}/resume", post(resume_agent))
        .route("/api/agents/{id}/close", post(close_agent))
        .route("/api/agents/{id}/archive", post(archive_agent))
        .route("/api/repos/{name}/notes", post(add_repo_note))
        .route("/api/notes", post(add_global_note))
        .route("/api/messages/{id}/catalogue-change", post(decide_catalogue_change))
        .route("/api/orchestrator/reset", post(reset_orchestrator))
        .route("/api/language", post(set_language))
        .route("/api/messages", post(post_message))
        .route("/hooks/{event}", post(run_hook))
        .route(
            "/mcp/agent/{agent_id}",
            post(agent_mcp)
                .get(mcp_method_not_allowed)
                .delete(mcp_method_not_allowed),
        )
        .route(
            "/mcp/orchestrator",
            post(orchestrator_mcp)
                .get(mcp_method_not_allowed)
                .delete(mcp_method_not_allowed),
        )
        .route("/ws", get(open_factory_socket))
        .route("/ws/terminal/{agent_id}", get(open_terminal_socket))
        .route_layer(axum::middleware::from_fn_with_state(Arc::clone(&state), token_only));
    Router::new()
        .route("/api/health", get(get_health))
        .merge(protected)
        .layer(axum::middleware::from_fn(local_only))
        .with_state(state)
}

#[cfg(test)]
mod tests;
