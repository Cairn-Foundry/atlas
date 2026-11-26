// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The orchestrator with a scripted model in place of `claude -p`.

use std::sync::Mutex;

use super::*;
use crate::contract::MessageKind;
use crate::testing::{Env, ids, settle};

/// A runner that records what it is asked and answers what the test scripted.
#[derive(Clone, Default)]
struct Script {
    runs: Arc<Mutex<Vec<ClaudeRun>>>,
    answers: Arc<Mutex<Vec<std::result::Result<ClaudeResult, String>>>>,
    /// When set, a run waits here until the test lets it go.
    gate: Option<Arc<tokio::sync::Semaphore>>,
}

impl Script {
    fn gated() -> Self {
        Self {
            gate: Some(Arc::new(tokio::sync::Semaphore::new(0))),
            ..Self::default()
        }
    }

    fn answer(&self, result: ClaudeResult) {
        self.answers.lock().expect("answers").push(Ok(result));
    }

    fn fail(&self, reason: &str) {
        self.answers.lock().expect("answers").push(Err(reason.to_owned()));
    }

    /// Lets `count` waiting runs go.
    fn release(&self, count: usize) {
        self.gate.as_ref().expect("a gated script").add_permits(count);
    }

    fn runs(&self) -> Vec<ClaudeRun> {
        self.runs.lock().expect("runs").clone()
    }

    fn runner(&self) -> ClaudeRunner {
        let script = self.clone();
        Arc::new(move |run| {
            let script = script.clone();
            Box::pin(async move {
                script.runs.lock().expect("runs").push(run);
                if let Some(gate) = &script.gate {
                    gate.acquire().await.expect("gate").forget();
                }
                let mut answers = script.answers.lock().expect("answers");
                if answers.is_empty() {
                    Ok(said("C'est noté.", "orch-1"))
                } else {
                    answers.remove(0)
                }
            })
        })
    }
}

fn said(text: &str, session_id: &str) -> ClaudeResult {
    ClaudeResult {
        text: text.to_owned(),
        session_id: Some(session_id.to_owned()),
        ..ClaudeResult::default()
    }
}

fn with_context(tokens: u64) -> ClaudeResult {
    let usage = StepUsage {
        input_tokens: tokens,
        output_tokens: 10,
        cache_read_tokens: tokens,
        cache_write_tokens: 0,
    };
    ClaudeResult {
        usage: Some(usage),
        turns: Some(2),
        duration_ms: Some(900),
        ..said("Réponse.", "orch-1")
    }
}

fn orchestrator(env: &Env, script: &Script) -> Orchestrator {
    Orchestrator::new(
        env.store.clone(),
        env.service.clone(),
        script.runner(),
        &env.config,
        env.clock.clock(),
    )
}

fn texts(env: &Env) -> Vec<String> {
    env.messages().into_iter().map(|message| message.text).collect()
}

#[tokio::test]
async fn keeps_one_chat_session_across_messages_and_posts_its_replies() {
    let (env, script) = (Env::new(), Script::default());
    let orchestrator = orchestrator(&env, &script);
    orchestrator.chat("Lance PROJ-1".to_owned(), Vec::new()).await;
    orchestrator.chat("Et PROJ-2 ?".to_owned(), Vec::new()).await;
    let runs = script.runs();
    assert_eq!(
        (runs[0].prompt.as_str(), runs[0].resume_id.as_deref()),
        ("Lance PROJ-1", None)
    );
    assert_eq!(
        (runs[1].prompt.as_str(), runs[1].resume_id.as_deref()),
        ("Et PROJ-2 ?", Some("orch-1"))
    );
    assert_eq!(texts(&env), ["C'est noté.", "C'est noté."]);
    assert!(!orchestrator.is_busy());
}

#[tokio::test]
async fn answers_in_its_own_words_when_the_run_says_nothing() {
    let (env, script) = (Env::new(), Script::default());
    script.answer(said("  ", "orch-1"));
    script.answer(ClaudeResult {
        is_error: true,
        ..said("", "orch-1")
    });
    let orchestrator = orchestrator(&env, &script);
    orchestrator.chat("a".to_owned(), Vec::new()).await;
    orchestrator.chat("b".to_owned(), Vec::new()).await;
    assert_eq!(texts(&env), ["C'est fait.", "Je n'ai pas pu traiter ce message."]);
}

#[tokio::test]
async fn gives_the_orchestrator_the_context_of_the_agents_mentioned_with_at() {
    let (env, script) = (Env::new(), Script::default());
    let agent = env.launch_default().await;
    let orchestrator = orchestrator(&env, &script);
    let mentions = vec![agent.id.clone(), "ghost".to_owned(), agent.id.clone()];
    orchestrator.chat("Où en est-il ?".to_owned(), mentions).await;
    assert_eq!(
        script.runs()[0].prompt,
        format!(
            "Où en est-il ?\n\nAgents mentioned:\n- PROJ-224 (Relances par e-mail): agentId {}, step Qualification, status working",
            agent.id
        )
    );
}

#[tokio::test]
async fn starts_a_fresh_session_after_a_reset_and_hides_the_old_thread() {
    let (env, script) = (Env::new(), Script::default());
    let orchestrator = orchestrator(&env, &script);
    let mut events = env.service.subscribe();
    orchestrator.chat("Avant".to_owned(), Vec::new()).await;
    env.clock.advance_minutes(1);
    orchestrator.reset().expect("reset");
    env.clock.advance_minutes(1);
    assert!(orchestrator.conversation().expect("conversation").is_empty());
    orchestrator.chat("Après".to_owned(), Vec::new()).await;
    assert_eq!(
        script.runs()[1],
        ClaudeRun {
            prompt: "Après".to_owned(),
            resume_id: None
        }
    );
    assert_eq!(orchestrator.conversation().expect("conversation").len(), 1);
    // The older messages stay in the registry.
    assert_eq!(env.messages().len(), 2);
    let mut has_reset = false;
    while let Ok(event) = events.try_recv() {
        has_reset |= matches!(event, Event::Reset);
    }
    assert!(has_reset);
}

#[tokio::test]
async fn refuses_to_reset_while_it_is_thinking_and_tells_the_clients_when_it_starts_and_stops() {
    let (env, script) = (Env::new(), Script::gated());
    let orchestrator = orchestrator(&env, &script);
    let mut events = env.service.subscribe();
    let thinking = tokio::spawn(orchestrator.chat("Longue demande".to_owned(), Vec::new()));
    assert!(orchestrator.is_busy());
    let Err(Error::Refusal(message)) = orchestrator.reset() else {
        panic!("reset accepted while busy");
    };
    assert_eq!(message, "l'orchestrateur réfléchit encore : réessaie après sa réponse");
    script.release(1);
    thinking.await.expect("join");
    assert!(!orchestrator.is_busy());
    let mut busy = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let Event::Busy(is_busy) = event {
            busy.push(is_busy);
        }
    }
    assert_eq!(busy, [true, false]);
}

#[tokio::test]
async fn runs_one_task_at_a_time() {
    let (env, script) = (Env::new(), Script::gated());
    let orchestrator = orchestrator(&env, &script);
    let first = tokio::spawn(orchestrator.chat("un".to_owned(), Vec::new()));
    let second = tokio::spawn(orchestrator.chat("deux".to_owned(), Vec::new()));
    settle().await;
    assert_eq!(script.runs().len(), 1);
    script.release(2);
    first.await.expect("first");
    second.await.expect("second");
    let prompts: Vec<String> = script.runs().into_iter().map(|run| run.prompt).collect();
    assert_eq!(prompts, ["un", "deux"]);
}

#[tokio::test]
async fn warns_the_master_when_a_run_fails_instead_of_crashing() {
    let (env, script) = (Env::new(), Script::default());
    script.fail("claude -p a échoué (code 1) : not logged in");
    let orchestrator = orchestrator(&env, &script);
    orchestrator.chat("Bonjour".to_owned(), Vec::new()).await;
    assert_eq!(
        texts(&env),
        ["L'orchestrateur a rencontré une erreur : claude -p a échoué (code 1) : not logged in"]
    );
    assert_eq!(env.notifier.sent().len(), 1);
    assert!(!orchestrator.is_busy());
}

#[tokio::test]
async fn describes_a_threshold_anomaly_with_what_it_may_do_in_a_one_shot_run() {
    let (env, script) = (Env::new(), Script::default());
    let agent = env.launch_default().await;
    let orchestrator = orchestrator(&env, &script);
    orchestrator.chat("Bonjour".to_owned(), Vec::new()).await;
    env.clock.advance_minutes(31);
    orchestrator
        .handle_anomaly(Anomaly::Threshold {
            agent_id: agent.id.clone(),
        })
        .await;
    let run = script.runs().remove(1);
    // Never in the chat session: the anomaly is a run of its own.
    assert_eq!(run.resume_id, None);
    let threshold = env.catalogue()["qualification"].threshold_minutes;
    assert!(run.prompt.starts_with(&format!(
        "Anomaly: the agent {} working on PROJ-224 (Relances par e-mail) has been in the step Qualification for 31 min, past the threshold of {threshold} min.\nSession open: yes.",
        agent.id
    )));
    assert!(run.prompt.ends_with("Do nothing else."));
}

#[tokio::test]
async fn sends_the_orchestrator_to_the_screen_of_an_idle_session_before_it_acts() {
    let (env, script) = (Env::new(), Script::default());
    let agent = env.launch_default().await;
    let prompt = orchestrator(&env, &script)
        .describe_anomaly(&Anomaly::Idle {
            agent_id: agent.id.clone(),
        })
        .expect("prompt");
    assert!(prompt.contains("waits for input during the step Qualification"));
    assert!(prompt.contains("Read its screen first with read_agent_terminal"));
}

#[tokio::test]
async fn describes_a_dead_session_with_its_exit_code() {
    let (env, script) = (Env::new(), Script::default());
    let agent = env.launch_default().await;
    env.service.close(&agent.id).expect("close");
    let orchestrator = orchestrator(&env, &script);
    let died = |exit_code| Anomaly::SessionDied {
        agent_id: agent.id.clone(),
        exit_code,
    };
    let prompt = orchestrator.describe_anomaly(&died(Some(137))).expect("prompt");
    assert!(prompt.contains("lost its session (exit code 137) during the step Qualification.\nSession open: no."));
    assert!(
        orchestrator
            .describe_anomaly(&died(None))
            .expect("prompt")
            .contains("(exit code unknown)")
    );
}

#[tokio::test]
async fn moves_the_chat_to_a_fresh_session_once_its_context_has_grown_too_large_with_the_last_exchanges() {
    let (env, script) = (Env::new(), Script::default());
    script.answer(with_context(env.config.orchestrator_max_context_tokens + 1));
    let orchestrator = orchestrator(&env, &script);
    let say = |text: &str| {
        env.clock.advance_ms(10);
        env.store
            .add_message(MessageRole::User, text, None, MessageExtras::default())
            .expect("message");
        orchestrator.chat(text.to_owned(), Vec::new())
    };
    say("Lance PROJ-1").await;
    say("Et maintenant ?").await;
    let rotated = script.runs().remove(1);
    assert_eq!(rotated.resume_id, None);
    assert!(
        rotated
            .prompt
            .starts_with("Last exchanges of this conversation, taken over in a new session.")
    );
    assert!(
        rotated
            .prompt
            .contains("Master: Lance PROJ-1\nYou: Réponse.\n\nMessage of the master:\nEt maintenant ?")
    );
    // What the run cost is kept, and its context size is the tokens of one turn.
    assert_eq!(
        env.store.get_value(CONTEXT_TOKENS_KEY).expect("value").as_deref(),
        Some("0")
    );
}

#[tokio::test]
async fn opens_a_fresh_chat_session_after_a_daemon_restart_the_thread_kept() {
    let (env, script) = (Env::new(), Script::default());
    let orchestrator = orchestrator(&env, &script);
    orchestrator.chat("Avant le redémarrage".to_owned(), Vec::new()).await;
    orchestrator.rotate_chat_session().expect("rotate");
    orchestrator.chat("Après".to_owned(), Vec::new()).await;
    assert_eq!(script.runs()[1].resume_id, None);
    assert_eq!(orchestrator.conversation().expect("conversation").len(), 2);
    // The session that answered is the one resumed from then on.
    orchestrator.chat("Encore".to_owned(), Vec::new()).await;
    assert_eq!(script.runs()[2].resume_id.as_deref(), Some("orch-1"));
}

#[tokio::test]
async fn decides_on_a_qualification_in_a_run_of_its_own_from_the_fields_the_agent_handed_over() {
    let (env, script) = (Env::new(), Script::gated());
    let orchestrator = orchestrator(&env, &script);
    orchestrator.listen();
    let agent = env.launch_default().await;
    let (service, agent_id, input) = (
        env.service.clone(),
        agent.id.clone(),
        env.fields_for("qualification", &[("size", "petit")]),
    );
    let closing = tokio::spawn(async move { service.complete_step(&agent_id, input).await });
    settle().await;
    let prompt = script.runs().remove(0).prompt;
    assert!(prompt.starts_with(&format!(
        "Validation requested: the agent {} on PROJ-224 (Relances par e-mail) hands in its step Qualification.\nNote of the master at launch: délai de 7 jours\nFields handed in:\n- need: need de qualification",
        agent.id
    )));
    assert!(prompt.contains("- size: petit"));
    assert!(prompt.contains("Steps that remain: plan, implementation, tests, review, correction, ship."));

    // The model decides through its tool while its run is still going.
    let remaining = ids(&["implementation", "tests", "review", "correction", "ship"]);
    env.service
        .approve_step(&agent.id, "Plan retiré : petit.", Some(&remaining))
        .expect("approve");
    script.release(1);
    closing.await.expect("join").expect("reply");
    settle().await;
    assert_eq!(env.agent(&agent.id).step_id, "implementation");
    let decisions = env
        .messages()
        .into_iter()
        .filter(|message| message.kind == MessageKind::Decision)
        .count();
    assert_eq!(decisions, 1);
}

#[tokio::test]
async fn keeps_the_workflow_when_its_run_ends_without_a_decision() {
    let (env, script) = (Env::new(), Script::default());
    let orchestrator = orchestrator(&env, &script);
    orchestrator.listen();
    let agent = env.launch_default().await;
    env.close_step(&agent.id).await;
    assert_eq!(env.agent(&agent.id).step_id, "plan");
    assert!(texts(&env).contains(&"Orchestrateur sans décision : étape validée, parcours inchangé.".to_owned()));
}

#[tokio::test]
async fn is_woken_by_a_session_that_dies() {
    let (env, script) = (Env::new(), Script::default());
    orchestrator(&env, &script).listen();
    let agent = env.launch_default().await;
    env.terminals.last().exit(Some(1));
    settle().await;
    assert!(
        script.runs()[0]
            .prompt
            .contains(&format!("the agent {} working on PROJ-224", agent.id))
    );
}

#[test]
fn reads_the_result_of_a_headless_run() {
    let stdout = r#"{"result":"Lancé.","session_id":"s1","is_error":false,"num_turns":3,"duration_ms":1200,"usage":{"input_tokens":10,"output_tokens":4,"cache_read_input_tokens":300,"cache_creation_input_tokens":20},"extra":true}"#;
    let result = parse_claude_output(stdout, true).expect("result");
    assert_eq!(
        (result.text.as_str(), result.session_id.as_deref(), result.is_error),
        ("Lancé.", Some("s1"), false)
    );
    assert_eq!(
        result.usage,
        Some(StepUsage {
            input_tokens: 10,
            output_tokens: 4,
            cache_read_tokens: 300,
            cache_write_tokens: 20
        })
    );
    assert_eq!((result.turns, result.duration_ms), (Some(3), Some(1200)));
}

#[test]
fn takes_the_exit_code_for_the_outcome_when_the_run_does_not_say() {
    assert!(parse_claude_output("{}", false).expect("result").is_error);
    assert!(!parse_claude_output("{}", true).expect("result").is_error);
    assert_eq!(parse_claude_output("Error: not logged in", false), None);
}

#[cfg(unix)]
#[tokio::test]
async fn gives_up_a_run_that_never_answers() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().expect("home");
    let command = home.path().join("claude");
    std::fs::write(&command, "#!/bin/sh\nsleep 30\n").expect("script");
    std::fs::set_permissions(&command, std::fs::Permissions::from_mode(0o755)).expect("mode");
    let config = AtlasConfig {
        claude_command: command.to_string_lossy().into_owned(),
        orchestrator_timeout_seconds: 1,
        ..AtlasConfig::default()
    };
    let runner = create_claude_runner(
        &config,
        home.path(),
        "http://127.0.0.1:1",
        "token",
        &LiveCatalogue::load(home.path()).expect("catalogue"),
    )
    .expect("runner");
    let outcome = runner(ClaudeRun {
        prompt: "bonjour".to_owned(),
        resume_id: None,
    })
    .await;
    assert_eq!(
        outcome.expect_err("timeout"),
        "claude -p n'a pas répondu en 1 s (orchestratorTimeoutSeconds)"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn describes_to_the_orchestrator_the_bricks_as_the_master_last_edited_them() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().expect("home");
    let command = home.path().join("claude");
    std::fs::write(&command, "#!/bin/sh\nexit 1\n").expect("script");
    std::fs::set_permissions(&command, std::fs::Permissions::from_mode(0o755)).expect("mode");
    let config = AtlasConfig {
        claude_command: command.to_string_lossy().into_owned(),
        ..AtlasConfig::default()
    };
    let catalogue = LiveCatalogue::load(home.path()).expect("catalogue");
    let runner = create_claude_runner(&config, home.path(), "http://127.0.0.1:1", "token", &catalogue).expect("runner");
    let prompt = || std::fs::read_to_string(home.path().join("orchestrator/system-prompt.md")).expect("prompt");
    assert!(prompt().contains("Reads the ticket and the code"));

    let brick = home.path().join("bricks/qualification.yml");
    let source = std::fs::read_to_string(&brick).expect("brick");
    std::fs::write(
        &brick,
        source.replace("Reads the ticket and the code", "Cadrage du ticket et du code"),
    )
    .expect("edit");
    catalogue.refresh();
    let _ = runner(ClaudeRun {
        prompt: "bonjour".to_owned(),
        resume_id: None,
    })
    .await;
    assert!(prompt().contains("Cadrage du ticket et du code"));
    // The same file tells the orchestrator which language to speak: it follows the one the master picks.
    assert!(prompt().contains("The master reads French."));
    crate::language::speak(crate::language::Language::En);
    let _ = runner(ClaudeRun {
        prompt: "hello".to_owned(),
        resume_id: None,
    })
    .await;
    assert!(prompt().contains("The master reads English."));
}
