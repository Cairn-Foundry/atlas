// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A whole factory for tests: a real git repository and registry, with fake terminals in place of
//! Claude Code sessions.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use foundry_pty::{PtyEvent, PtySize};
use tokio::sync::mpsc;

use crate::catalogue::{Catalogue, LiveCatalogue};
use crate::clock::tests::TestClock;
use crate::config::{AtlasConfig, parse_config};
use crate::contract::{ChatMessage, CreateAgentInput};
use crate::notifier::Notifier;
use crate::service::{AgentService, CompleteStepInput, FieldValue, ServiceDeps, Wakeup};
use crate::store::{AgentRecord, Store};
use crate::terminal::{SpawnRequest, TerminalLauncher, TerminalProcess, TerminalSink};

pub(crate) const TOKEN: &str = "secret";
pub(crate) const DAEMON_URL: &str = "http://127.0.0.1:4317";
pub(crate) const HOOK_EXECUTABLE: &str = "/opt/atlas/atlas-daemon";
const CONFIG: &str = "agentArgs: [\"--effort\", \"high\"]\napprovalTimeoutSeconds: 1\n";
/// Long enough for the timers a closed step arms (the session switch) to run.
const TIMER_SLACK: Duration = Duration::from_millis(250);

/// A session started by the service, as a test sees and drives it.
pub(crate) struct FakeSession {
    pub(crate) request: SpawnRequest,
    sink: TerminalSink,
    written: Mutex<String>,
    is_killed: AtomicBool,
}

impl FakeSession {
    /// Value following `flag` in the spawn arguments.
    pub(crate) fn arg(&self, flag: &str) -> Option<&str> {
        let index = self.request.args.iter().position(|argument| argument == flag)?;
        self.request.args.get(index + 1).map(String::as_str)
    }

    /// The prompt the session was started with: its last argument.
    pub(crate) fn prompt(&self) -> &str {
        self.request.args.last().map_or("", String::as_str)
    }

    pub(crate) fn env(&self, name: &str) -> Option<&str> {
        self.request
            .env
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub(crate) fn written(&self) -> String {
        self.written.lock().expect("written").clone()
    }

    pub(crate) fn is_killed(&self) -> bool {
        self.is_killed.load(Ordering::SeqCst)
    }

    pub(crate) fn print(&self, data: &str) {
        self.print_bytes(data.as_bytes());
    }

    pub(crate) fn print_bytes(&self, data: &[u8]) {
        (self.sink)(PtyEvent::Data(data.to_vec()));
    }

    /// The process ends by itself.
    pub(crate) fn exit(&self, code: Option<i32>) {
        (self.sink)(PtyEvent::Exit(code));
    }
}

struct FakeProcess(Arc<FakeSession>);

impl TerminalProcess for FakeProcess {
    fn write(&self, data: &[u8]) {
        self.0
            .written
            .lock()
            .expect("written")
            .push_str(&String::from_utf8_lossy(data));
    }

    fn resize(&self, _size: PtySize) {}

    fn kill(&self) {
        if !self.0.is_killed.swap(true, Ordering::SeqCst) {
            self.0.exit(Some(0));
        }
    }
}

#[derive(Default)]
pub(crate) struct FakeTerminals {
    sessions: Mutex<Vec<Arc<FakeSession>>>,
    /// When set, the next spawn fails with this reason.
    pub(crate) failure: Mutex<Option<String>>,
}

impl FakeTerminals {
    pub(crate) fn count(&self) -> usize {
        self.sessions.lock().expect("sessions").len()
    }

    pub(crate) fn last(&self) -> Arc<FakeSession> {
        Arc::clone(
            self.sessions
                .lock()
                .expect("sessions")
                .last()
                .expect("a session was started"),
        )
    }
}

struct FakeLauncher(Arc<FakeTerminals>);

impl TerminalLauncher for FakeLauncher {
    fn spawn(&self, request: &SpawnRequest, sink: TerminalSink) -> Result<Box<dyn TerminalProcess>, String> {
        if let Some(reason) = self.0.failure.lock().expect("failure").take() {
            return Err(reason);
        }
        let session = Arc::new(FakeSession {
            request: request.clone(),
            sink,
            written: Mutex::new(String::new()),
            is_killed: AtomicBool::new(false),
        });
        self.0.sessions.lock().expect("sessions").push(Arc::clone(&session));
        Ok(Box::new(FakeProcess(session)))
    }
}

#[derive(Clone, Default)]
pub(crate) struct RecordingNotifier(Arc<Mutex<Vec<String>>>);

impl RecordingNotifier {
    /// Body of each notification shown.
    pub(crate) fn sent(&self) -> Vec<String> {
        self.0.lock().expect("notifications").clone()
    }
}

impl Notifier for RecordingNotifier {
    fn notify(&self, _title: &str, body: &str) {
        self.0.lock().expect("notifications").push(body.to_owned());
    }
}

pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A repository on `main` with one commit.
pub(crate) fn init_repository(path: &Path) {
    std::fs::create_dir_all(path).expect("repository folder");
    git(path, &["init", "--quiet", "--initial-branch=main"]);
    git(path, &["config", "user.email", "atlas@example.test"]);
    git(path, &["config", "user.name", "Atlas"]);
    git(path, &["commit", "--quiet", "--allow-empty", "-m", "init"]);
}

pub(crate) struct Env {
    _root: tempfile::TempDir,
    pub(crate) home: PathBuf,
    /// The repository agents are launched in.
    pub(crate) repo: PathBuf,
    pub(crate) claude_dir: PathBuf,
    pub(crate) clock: TestClock,
    pub(crate) store: Store,
    pub(crate) service: AgentService,
    pub(crate) terminals: Arc<FakeTerminals>,
    pub(crate) notifier: RecordingNotifier,
    pub(crate) config: AtlasConfig,
}

impl Env {
    pub(crate) fn new() -> Self {
        let root = tempfile::tempdir().expect("root");
        let repo = root.path().join("repos").join("shop");
        init_repository(&repo);
        // Every folder hangs from the path git reports for the repository: links resolved, as the
        // service compares them, and on Windows the form git writes rather than the verbatim one
        // `canonicalize` gives and git refuses.
        let repo = PathBuf::from(git(&repo, &["rev-parse", "--show-toplevel"]));
        let base = repo.ancestors().nth(2).expect("root").to_path_buf();
        let (home, claude_dir) = (base.join("home"), base.join("claude"));
        std::fs::create_dir_all(&home).expect("home");
        let clock = TestClock::new();
        let store = Store::in_memory(clock.clock()).expect("store");
        let terminals = Arc::new(FakeTerminals::default());
        let notifier = RecordingNotifier::default();
        let config = parse_config(CONFIG, "config.yml").expect("config");
        let service = AgentService::new(ServiceDeps {
            store: store.clone(),
            catalogue: LiveCatalogue::load(&home).expect("catalogue"),
            config: config.clone(),
            home: home.clone(),
            daemon_url: DAEMON_URL.to_owned(),
            token: TOKEN.to_owned(),
            hook_executable: HOOK_EXECUTABLE.to_owned(),
            launcher: Box::new(FakeLauncher(Arc::clone(&terminals))),
            notifier: Box::new(notifier.clone()),
            claude_dir: claude_dir.clone(),
            claude_state_path: base.join("claude.json"),
            environment: vec![
                ("PATH".to_owned(), "/usr/bin".to_owned()),
                ("CLAUDECODE".to_owned(), "1".to_owned()),
            ],
            now: clock.clock(),
            close_delay: Duration::ZERO,
        });
        Self {
            _root: root,
            home,
            repo,
            claude_dir,
            clock,
            store,
            service,
            terminals,
            notifier,
            config,
        }
    }

    pub(crate) fn catalogue(&self) -> Arc<Catalogue> {
        self.service.catalogue()
    }

    pub(crate) fn repo_path(&self) -> String {
        self.repo.to_string_lossy().into_owned()
    }

    /// What launches the usual agent of the tests; a test changes the fields it is about.
    pub(crate) fn launch_input(&self) -> CreateAgentInput {
        CreateAgentInput {
            ticket_ref: "PROJ-224".to_owned(),
            title: Some("Relances par e-mail".to_owned()),
            repo_path: Some(self.repo_path()),
            note: Some("délai de 7 jours".to_owned()),
            ..CreateAgentInput::default()
        }
    }

    pub(crate) async fn launch(&self, input: CreateAgentInput) -> AgentRecord {
        self.service.launch(input).await.expect("launch")
    }

    pub(crate) async fn launch_default(&self) -> AgentRecord {
        self.launch(self.launch_input()).await
    }

    pub(crate) async fn launch_with(&self, steps: &[&str]) -> AgentRecord {
        self.launch(CreateAgentInput {
            steps: Some(ids(steps)),
            ..self.launch_input()
        })
        .await
    }

    pub(crate) fn agent(&self, agent_id: &str) -> AgentRecord {
        self.store.require_agent(agent_id).expect("agent")
    }

    pub(crate) fn messages(&self) -> Vec<ChatMessage> {
        self.store.list_messages(None).expect("messages")
    }

    pub(crate) fn last_message(&self) -> ChatMessage {
        self.messages().pop().expect("a message")
    }

    /// Has the service wake this test in place of the orchestrator.
    pub(crate) fn wakeups(&self) -> mpsc::UnboundedReceiver<Wakeup> {
        let (sender, receiver) = mpsc::unbounded_channel();
        self.service.wake(sender);
        receiver
    }

    /// Fields that satisfy a brick of the shipped catalogue, so a test can close any step.
    pub(crate) fn fields_for(&self, brick_id: &str, overrides: &[(&str, &str)]) -> CompleteStepInput {
        let mut fields: std::collections::HashMap<String, FieldValue> = self.catalogue()[brick_id]
            .writes
            .keys()
            .map(|field| (field.clone(), FieldValue::Text(format!("{field} de {brick_id}"))))
            .collect();
        for (field, value) in overrides {
            fields.insert((*field).to_owned(), FieldValue::Text((*value).to_owned()));
        }
        CompleteStepInput {
            fields,
            findings_above_threshold: None,
        }
    }

    /// Closes the agent's current step the way a well-behaved agent would, then lets the session switch run.
    pub(crate) async fn close_step(&self, agent_id: &str) -> String {
        self.close_review_step(agent_id, 0).await
    }

    /// Like `close_step`, with the findings a review pass reports.
    pub(crate) async fn close_review_step(&self, agent_id: &str, findings: u32) -> String {
        let agent = self.agent(agent_id);
        if agent.step_id == "plan" {
            self.service.ask_user(agent_id, "Plan ok ?").expect("ask");
            self.service.on_user_prompt(agent_id, None).expect("answer");
        }
        let overrides: &[(&str, &str)] = if agent.step_id == "ship" {
            &[("mrUrl", "https://gitlab.example/mr/1")]
        } else {
            &[]
        };
        let mut input = self.fields_for(&agent.step_id, overrides);
        if agent.step_id == "review" {
            input.findings_above_threshold = Some(findings);
        }
        let reply = self
            .service
            .complete_step(agent_id, input)
            .await
            .expect("complete_step");
        settle().await;
        reply
    }

    pub(crate) async fn walk_to(&self, agent_id: &str, brick_id: &str) {
        for _ in 0..12 {
            if self.agent(agent_id).step_id == brick_id {
                return;
            }
            self.close_step(agent_id).await;
        }
        panic!("étape {brick_id} non atteinte");
    }

    /// The agent's workflow as `brick:state`, a later review pass as `brick#pass:state`.
    pub(crate) fn path(&self, agent_id: &str) -> Vec<String> {
        self.agent(agent_id)
            .steps
            .iter()
            .map(|step| {
                let pass = if step.pass > 1 {
                    format!("#{}", step.pass)
                } else {
                    String::new()
                };
                format!("{}{pass}:{}", step.brick_id, step.state.as_str())
            })
            .collect()
    }
}

pub(crate) fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// Lets the timers the service armed, and what the fake terminals reported, run.
pub(crate) async fn settle() {
    tokio::time::sleep(TIMER_SLACK).await;
}

/// The message of an expected refusal; anything else fails the test.
pub(crate) fn refusal<T: std::fmt::Debug>(result: crate::error::Result<T>) -> String {
    match result {
        Err(crate::error::Error::Refusal(message)) => message,
        other => panic!("refus attendu, obtenu {other:?}"),
    }
}
