// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The life of an agent, driven the way the daemon drives it: real git, real registry, fake terminals.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use super::sessions::{PASTE_SUBMIT_DELAY, quote_argument};
use super::steps::field_text;
use super::*;
use crate::contract::{CatalogueChange, CatalogueDecision, CreateAgentInput, RepoInput, StepState, StepUsage};
use crate::prompts::{NO_BRANCH_NOTICE, RESUME_PROMPT};
use crate::testing::{DAEMON_URL, Env, git, ids, init_repository, refusal, settle};

const DEFAULT_BRANCH: &str = "feat/PROJ-224/relancesParEMail";

/// Lets what a fake terminal just reported reach the service.
async fn tick() {
    tokio::time::sleep(Duration::from_millis(20)).await;
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("file")).expect("json")
}

fn request_id(wakeup: &Wakeup) -> u64 {
    match wakeup {
        Wakeup::Approval(request) => request.id,
        Wakeup::Anomaly(anomaly) => panic!("not an approval request: {anomaly:?}"),
    }
}

fn text(value: &str) -> FieldValue {
    FieldValue::Text(value.to_owned())
}

mod launching_an_agent {
    use super::*;

    #[tokio::test]
    async fn creates_a_worktree_on_a_named_branch_and_starts_claude_code_with_the_atlas_wiring() {
        let env = Env::new();
        let input = CreateAgentInput {
            branch_type: Some("fix".to_owned()),
            branch_name: Some("renameShopAccount".to_owned()),
            ..env.launch_input()
        };
        let agent = env.launch(input).await;
        assert_eq!(
            (agent.branch.as_str(), agent.base_branch.as_str()),
            ("fix/PROJ-224/renameShopAccount", "main")
        );
        assert!(agent.owns_worktree);
        assert_eq!(agent.step_id, "qualification");
        let worktree = Path::new(&agent.worktree_path);
        assert_eq!(worktree.parent(), Some(env.home.join("worktrees").as_path()));
        assert!(agent.worktree_path.contains("proj-224-"));
        assert_eq!(
            git(Path::new(&agent.worktree_path), &["rev-parse", "--abbrev-ref", "HEAD"]),
            agent.branch
        );

        let session = env.terminals.last();
        assert_eq!(session.request.command, "claude");
        assert_eq!(session.request.cwd, agent.worktree_path);
        assert_eq!(session.arg("--session-id"), agent.live_session_id.as_deref());
        assert_eq!(session.arg("--model"), Some("sonnet"));
        assert_eq!(session.arg("--allowedTools"), Some("mcp__atlas"));
        assert!(
            session
                .arg("--disallowedTools")
                .expect("withheld")
                .split(',')
                .any(|tool| tool == "Agent")
        );
        assert_eq!(session.arg("--permission-mode"), Some("bypassPermissions"));
        assert_eq!(session.arg("--effort"), Some("high"));
        assert_eq!(session.request.args[session.request.args.len() - 2], "--");
        assert!(session.prompt().contains("PROJ-224") && !session.prompt().contains('"'));
        assert_eq!(
            (session.env("ATLAS_AGENT_ID"), session.env("ATLAS_TOKEN")),
            (Some(agent.id.as_str()), Some("secret"))
        );
        assert_eq!(session.env("ATLAS_SESSION_ID"), agent.live_session_id.as_deref());
        assert_eq!(
            (session.env("PATH"), session.env("CLAUDECODE")),
            (Some("/usr/bin"), None)
        );
    }

    #[tokio::test]
    async fn marks_the_repository_trusted_for_claude_code() {
        let env = Env::new();
        env.launch_default().await;
        let state = read_json(&env.home.parent().expect("root").join("claude.json"));
        assert_eq!(state["projects"][env.repo_path()]["hasTrustDialogAccepted"], true);
    }

    #[tokio::test]
    async fn gives_the_agent_the_default_workflow_and_tells_the_master_in_a_launch_card() {
        let env = Env::new();
        let agent = env.launch_default().await;
        let bricks: Vec<&str> = agent.steps.iter().map(|step| step.brick_id.as_str()).collect();
        assert_eq!(bricks, env.config.default_steps);
        let message = env.last_message();
        assert_eq!(
            (message.kind, message.text.as_str()),
            (MessageKind::Launch, "Lancé avec le parcours par défaut.")
        );
        assert_eq!(message.agent_id.as_deref(), Some(agent.id.as_str()));
        let card = message.card.expect("card");
        assert_eq!(card.ticket_ref, "PROJ-224");
        assert_eq!(
            card.steps[0],
            CardStep {
                name: "Qualification".to_owned(),
                state: StepState::Current
            }
        );
        assert_eq!(message.actions, [MessageAction::Session]);
    }

    #[tokio::test]
    async fn starts_where_the_orchestrator_says_the_earlier_bricks_recorded_as_done_outside_atlas() {
        let env = Env::new();
        let input = CreateAgentInput {
            steps: Some(ids(&["qualification", "implementation", "tests"])),
            start_at: Some("implementation".to_owned()),
            reason: Some("Code déjà qualifié.".to_owned()),
            ..env.launch_input()
        };
        let agent = env.launch(input).await;
        assert_eq!(
            env.path(&agent.id),
            ["qualification:external", "implementation:current", "tests:upcoming"]
        );
        assert_eq!(env.last_message().text, "Code déjà qualifié.");
    }

    #[tokio::test]
    async fn refuses_a_workflow_that_breaks_a_dependency_names_an_unknown_brick_or_starts_off_the_list() {
        let env = Env::new();
        let with = |steps: &[&str], start_at: Option<&str>| CreateAgentInput {
            steps: Some(ids(steps)),
            start_at: start_at.map(str::to_owned),
            ..env.launch_input()
        };
        assert_eq!(
            refusal(env.service.launch(with(&["implementation"], None)).await),
            "parcours refusé : implementation doit venir après qualification"
        );
        assert!(
            refusal(env.service.launch(with(&["ghost"], None)).await)
                .starts_with("parcours refusé : brique inconnue : ghost.")
        );
        assert_eq!(
            refusal(env.service.launch(with(&["tache"], Some("ship"))).await),
            "startAt (ship) ne figure pas dans le parcours"
        );
        assert_eq!(env.terminals.count(), 0);
        assert!(env.store.list_agents(true).expect("agents").is_empty());
    }

    #[tokio::test]
    async fn works_on_an_existing_branch_in_an_existing_worktree_or_in_the_project_folder_when_asked() {
        let env = Env::new();
        git(&env.repo, &["branch", "fix/existing"]);
        let on_branch = env
            .launch(CreateAgentInput {
                branch: Some("fix/existing".to_owned()),
                ..env.launch_input()
            })
            .await;
        assert_eq!(
            (on_branch.branch.as_str(), on_branch.owns_worktree),
            ("fix/existing", true)
        );
        env.service.archive(&on_branch.id).expect("archive");

        let in_worktree = env
            .launch(CreateAgentInput {
                worktree_path: Some(on_branch.worktree_path.clone()),
                ..env.launch_input()
            })
            .await;
        assert_eq!(in_worktree.worktree_path, on_branch.worktree_path);
        assert_eq!(
            (in_worktree.branch.as_str(), in_worktree.owns_worktree),
            ("fix/existing", false)
        );

        let in_place = env
            .launch(CreateAgentInput {
                in_place: Some(true),
                ..env.launch_input()
            })
            .await;
        assert_eq!(
            (
                in_place.worktree_path.as_str(),
                in_place.branch.as_str(),
                in_place.owns_worktree
            ),
            (env.repo_path().as_str(), "main", false)
        );
    }

    #[tokio::test]
    async fn never_puts_two_active_agents_in_the_same_folder() {
        let env = Env::new();
        let in_place = || CreateAgentInput {
            in_place: Some(true),
            ..env.launch_input()
        };
        env.launch(in_place()).await;
        let message = refusal(env.service.launch(in_place()).await);
        assert_eq!(
            message,
            format!(
                "PROJ-224 travaille déjà dans {} : deux agents ne partagent pas un dossier",
                env.repo_path()
            )
        );
    }

    #[tokio::test]
    async fn words_a_git_refusal_for_the_orchestrator() {
        let env = Env::new();
        let on_main = CreateAgentInput {
            branch: Some("main".to_owned()),
            ..env.launch_input()
        };
        assert_eq!(
            refusal(env.service.launch(on_main).await),
            "préparation du dossier de travail impossible : main est ouverte dans le dossier principal du projet : le maître doit demander explicitement d'y travailler (inPlace)"
        );
    }

    #[tokio::test]
    async fn reviews_a_merge_request_from_its_link_and_asks_for_it_when_it_is_missing() {
        let env = Env::new();
        let review = |mr_url: Option<&str>| CreateAgentInput {
            steps: Some(ids(&["review-mr"])),
            mr_url: mr_url.map(str::to_owned),
            ..env.launch_input()
        };
        assert_eq!(
            refusal(env.service.launch(review(None)).await),
            "ce parcours relit une merge request : donne son lien dans mrUrl"
        );
        let agent = env.launch(review(Some("https://gitlab.example/mr/9"))).await;
        // Work that never commits gets a folder, not a branch of its own.
        assert_eq!(agent.branch, "");
        assert_eq!(
            git(Path::new(&agent.worktree_path), &["rev-parse", "--abbrev-ref", "HEAD"]),
            "HEAD"
        );
        assert!(
            env.terminals
                .last()
                .prompt()
                .starts_with("/deep-review https://gitlab.example/mr/9 post")
        );
        assert!(
            !env.terminals
                .last()
                .arg("--disallowedTools")
                .expect("withheld")
                .contains("Agent,")
        );
    }

    #[tokio::test]
    async fn runs_the_free_task_on_the_model_the_orchestrator_picked() {
        let env = Env::new();
        let input = CreateAgentInput {
            steps: Some(ids(&["tache"])),
            task_model: Some("opus".to_owned()),
            ..env.launch_input()
        };
        let agent = env.launch(input).await;
        assert_eq!(agent.steps[0].model, "opus");
        assert_eq!(env.terminals.last().arg("--model"), Some("opus"));
    }

    #[tokio::test]
    async fn points_the_mcp_config_at_the_agent_url_with_the_token_and_wires_every_hook() {
        let env = Env::new();
        let agent = env.launch_default().await;
        let session = env.terminals.last();
        let mcp = read_json(Path::new(session.arg("--mcp-config").expect("mcp config")));
        assert_eq!(
            mcp["mcpServers"]["atlas"]["url"],
            format!("{DAEMON_URL}/mcp/agent/{}", agent.id)
        );
        assert_eq!(mcp["mcpServers"]["atlas"]["headers"]["Authorization"], "Bearer secret");
        let settings = read_json(Path::new(session.arg("--settings").expect("settings")));
        let hooks = settings["hooks"].as_object().expect("hooks");
        assert_eq!(
            hooks.keys().collect::<Vec<_>>(),
            [
                "Stop",
                "Notification",
                "PostToolUse",
                "SessionStart",
                "UserPromptSubmit"
            ]
        );
        assert_eq!(
            hooks["SessionStart"][0]["hooks"][0]["command"],
            "\"/opt/atlas/atlas-daemon\" hook session-start"
        );
    }

    #[tokio::test]
    async fn names_a_new_branch_from_the_pattern_the_ticket_and_the_title() {
        let env = Env::new();
        let input = CreateAgentInput {
            ticket_ref: "SHOP 12 / été".to_owned(),
            title: Some("Corriger l'envoi".to_owned()),
            ..env.launch_input()
        };
        assert_eq!(env.launch(input).await.branch, "feat/SHOP-12-t-/corrigerLEnvoi");
    }

    #[tokio::test]
    async fn takes_a_brick_the_master_edited_without_a_restart_and_refuses_one_that_does_not_hold() {
        let env = Env::new();
        let brick = env.home.join("bricks/qualification.yml");
        let source = std::fs::read_to_string(&brick).expect("brick");
        std::fs::write(&brick, source.replace("name: Qualification", "name: Cadrage")).expect("edit");
        let agent = env.launch_default().await;
        assert_eq!(agent.steps[0].name, "Cadrage");

        std::fs::write(&brick, "id: qualification\n").expect("broken edit");
        let other = env
            .launch(CreateAgentInput {
                ticket_ref: "PROJ-225".to_owned(),
                ..env.launch_input()
            })
            .await;
        assert_eq!(other.steps[0].name, "Cadrage");
        let warnings: Vec<String> = env
            .messages()
            .into_iter()
            .map(|message| message.text)
            .filter(|text| text.starts_with("Les briques modifiées ne sont pas prises en compte"))
            .collect();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("qualification.yml"), "{}", warnings[0]);
    }

    #[tokio::test]
    async fn blocks_the_agent_with_a_way_to_resume_when_claude_code_cannot_start() {
        let env = Env::new();
        *env.terminals.failure.lock().expect("failure") =
            Some("lancement de claude impossible : introuvable".to_owned());
        let agent = env.service.launch(env.launch_input()).await.expect("launch");
        assert_eq!(
            (agent.status, agent.status_reason.as_deref(), agent.live_session_id),
            (
                AgentStatus::Blocked,
                Some("lancement de claude impossible : introuvable"),
                None
            )
        );
        assert!(!env.service.has_live_session(&agent.id));
        assert_eq!(env.last_message().actions, [MessageAction::Resume]);
        // Claude Code is back: the agent picks its first step up in the folder made for it.
        *env.terminals.failure.lock().expect("failure") = None;
        let resumed = env.service.resume(&agent.id, None).expect("resume");
        assert_eq!(resumed.status, AgentStatus::Working);
        assert!(env.service.has_live_session(&agent.id));
    }
}

mod the_prompts_along_the_chain {
    use super::*;

    #[tokio::test]
    async fn tells_a_session_who_it_is_its_whole_path_and_the_standing_rules() {
        let env = Env::new();
        let agent = env.launch_default().await;
        let brief = env.service.on_session_start(&agent.id).expect("brief");
        assert!(brief.starts_with("You are the Atlas agent in charge of PROJ-224 (Relances par e-mail)."));
        assert!(brief.contains(&format!("branch {DEFAULT_BRANCH} (base main)")));
        assert!(
            brief.contains(
                "Your workflow: [Qualification] > Plan > Implementation > Tests > Review > Correction > Ship"
            )
        );
        assert!(brief.contains("How to work in Atlas:"));
    }

    #[tokio::test]
    async fn serves_a_step_with_its_instructions_the_fields_to_hand_over_and_only_the_sections_it_reads() {
        let env = Env::new();
        let agent = env.launch_default().await;
        let first = env.service.next_step(&agent.id).expect("brief");
        assert!(first.starts_with("Step: Qualification\n"));
        assert!(first.contains("## Note of the master\ndélai de 7 jours"));
        assert!(first.contains("- need: "));
        env.walk_to(&agent.id, "tests").await;
        let tests = env.service.next_step(&agent.id).expect("brief");
        assert!(tests.contains("## Qualification\nneed: need de qualification"));
        // The tests brick does not read the plan.
        assert!(!tests.contains("## Plan"));
    }

    #[tokio::test]
    async fn refuses_a_step_closed_without_every_field_its_brick_asks_for() {
        let env = Env::new();
        let agent = env.launch_default().await;
        let mut input = env.fields_for("qualification", &[("need", "  ")]);
        input.fields.remove("size");
        let message = refusal(env.service.complete_step(&agent.id, input).await);
        assert_eq!(
            message,
            "champs manquants dans fields : need, size. Une liste vide s'écrit \"none\"."
        );
        assert_eq!(env.agent(&agent.id).step_id, "qualification");
    }
}

mod the_language_of_the_factory {
    use super::*;
    use crate::language::{self, Language};

    #[tokio::test]
    async fn speaks_the_language_the_master_picks_to_him_and_has_the_agents_do_the_same() {
        let env = Env::new();
        let agent = env.launch_default().await;
        let mut events = env.service.subscribe();
        assert!(
            env.service
                .on_session_start(&agent.id)
                .expect("brief")
                .contains("The master reads French: write to him in French")
        );

        env.service.set_language(Language::En).expect("language");
        assert!(matches!(events.try_recv(), Ok(Event::Language)));
        assert_eq!(language::picked(&env.store).expect("picked"), Some(Language::En));
        // What only an agent reads stays as it was written; the rule that makes it speak follows.
        let brief = env.service.next_step(&agent.id).expect("brief");
        assert!(brief.starts_with("Step: Qualification\n"));
        assert!(brief.ends_with(
            "The master reads English: write to him in English (what you say in this terminal, your questions, a plan or a report he reads). Code, commits and merge requests follow the conventions of the repository."
        ));
        // What Atlas writes for the master is in his language from then on.
        env.service.ask_user(&agent.id, "Question").expect("ask");
        let warning = env
            .store
            .list_messages(None)
            .expect("messages")
            .pop()
            .expect("a message");
        assert_eq!(warning.text, "The agent working on PROJ-224 needs your help.");
        let refused = refusal(env.service.complete_step(&agent.id, CompleteStepInput::default()).await);
        assert_eq!(
            refused,
            "you asked the master a question with ask_user: wait for his answer in this terminal before closing the step"
        );

        env.service.set_language(Language::Fr).expect("language");
        assert_eq!(language::picked(&env.store).expect("picked"), Some(Language::Fr));
        let refused = refusal(env.service.complete_step(&agent.id, CompleteStepInput::default()).await);
        assert!(refused.starts_with("tu as posé une question au maître"), "{refused}");
    }

    #[tokio::test]
    async fn follows_the_system_until_the_master_picks_a_language() {
        let env = Env::new();
        assert_eq!(language::picked(&env.store).expect("picked"), None);
        env.store.set_value("language", "klingon").expect("value");
        assert_eq!(language::picked(&env.store).expect("picked"), None);
    }
}

mod moving_through_the_workflow {
    use super::*;

    #[tokio::test]
    async fn opens_a_fresh_session_when_the_model_or_the_group_changes_and_shares_it_otherwise() {
        let env = Env::new();
        let agent = env.launch_default().await;
        let first = env.terminals.last();
        assert!(env.close_step(&agent.id).await.contains("goes to another session"));
        assert!(first.is_killed());
        assert_eq!(env.terminals.count(), 2);
        assert_eq!(env.terminals.last().arg("--model"), Some("opus"));

        env.close_step(&agent.id).await;
        assert_eq!(env.terminals.count(), 3);
        assert_eq!(
            (
                env.terminals.last().arg("--model"),
                env.terminals.last().arg("--resume")
            ),
            (Some("sonnet"), None)
        );

        let carried_on = env.close_step(&agent.id).await;
        assert!(carried_on.starts_with("Step Implementation closed. Go straight on to the next one, in this session."));
        assert!(carried_on.contains("Step: Tests") && carried_on.contains("- result: "));
        assert_eq!(env.terminals.count(), 3);
        let on_tests = env.agent(&agent.id);
        assert_eq!(
            (on_tests.step_id.as_str(), on_tests.status),
            ("tests", AgentStatus::Working)
        );
        assert_eq!(on_tests.steps[3].session_id, on_tests.steps[2].session_id);
    }

    #[tokio::test]
    async fn records_what_each_step_produced_how_long_it_took_and_what_it_cost() {
        let env = Env::new();
        let agent = env.launch_default().await;
        let session_id = agent.live_session_id.clone().expect("session");
        let project = env.claude_dir.join("projects/-repo");
        std::fs::create_dir_all(&project).expect("transcripts");
        let line = r#"{"type":"assistant","timestamp":"2026-01-01T10:04:00.000Z","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":20,"cache_read_input_tokens":300,"cache_creation_input_tokens":40}}}"#;
        std::fs::write(project.join(format!("{session_id}.jsonl")), line).expect("transcript");
        env.clock.advance_minutes(8);
        env.close_step(&agent.id).await;

        let qualification = env.agent(&agent.id).steps.remove(0);
        assert_eq!(qualification.state, StepState::Done);
        assert_eq!(qualification.started_at.as_deref(), Some("2026-01-01T10:00:00.000Z"));
        assert_eq!(qualification.ended_at.as_deref(), Some("2026-01-01T10:08:00.000Z"));
        assert_eq!(
            qualification.usage,
            Some(StepUsage {
                input_tokens: 10,
                output_tokens: 20,
                cache_read_tokens: 300,
                cache_write_tokens: 40
            })
        );
        assert_eq!(qualification.head_sha, Some(git(&env.repo, &["rev-parse", "HEAD"])));
        let output = qualification.output.expect("output");
        assert_eq!(output["need"], "need de qualification");
        assert_eq!(
            output.keys().collect::<Vec<_>>(),
            env.catalogue()["qualification"].writes.keys().collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn takes_a_list_or_a_number_where_a_field_asks_for_text() {
        let env = Env::new();
        let agent = env.launch_default().await;
        let mut input = env.fields_for("qualification", &[]);
        input.fields.insert("risks".to_owned(), FieldValue::List(Vec::new()));
        input.fields.insert(
            "questions".to_owned(),
            FieldValue::List(ids(&["Quel délai ?", "Quelle langue ?"])),
        );
        input.fields.insert("size".to_owned(), FieldValue::Number(3.into()));
        env.service.complete_step(&agent.id, input).await.expect("complete");
        let output = env.agent(&agent.id).steps.remove(0).output.expect("output");
        assert_eq!(
            (
                output["risks"].as_str(),
                output["questions"].as_str(),
                output["size"].as_str()
            ),
            ("none", "- Quel délai ?\n- Quelle langue ?", "3")
        );
    }

    #[tokio::test]
    async fn refuses_to_close_the_plan_before_the_master_was_asked() {
        let env = Env::new();
        let agent = env.launch_default().await;
        env.walk_to(&agent.id, "plan").await;
        let message = refusal(env.service.complete_step(&agent.id, env.fields_for("plan", &[])).await);
        assert_eq!(
            message,
            "l'étape Plan demande la validation du maître : soumets-la avec ask_user et attends sa réponse"
        );
        assert!(
            env.close_step(&agent.id)
                .await
                .contains("The step Implementation goes to another session")
        );
    }

    #[tokio::test]
    async fn refuses_to_close_a_step_that_changes_code_while_something_is_not_committed() {
        let env = Env::new();
        let agent = env.launch_default().await;
        env.walk_to(&agent.id, "implementation").await;
        let worktree = Path::new(&agent.worktree_path);
        std::fs::write(worktree.join("draft.txt"), "work").expect("draft");
        let message = refusal(
            env.service
                .complete_step(&agent.id, env.fields_for("implementation", &[]))
                .await,
        );
        assert!(message.starts_with(&format!(
            "git status n'est pas propre dans {} : commite ton travail",
            agent.worktree_path
        )));
        assert_eq!(env.agent(&agent.id).step_id, "implementation");

        git(worktree, &["add", "-A"]);
        git(
            worktree,
            &[
                "-c",
                "user.email=a@b.test",
                "-c",
                "user.name=A",
                "commit",
                "--quiet",
                "-m",
                "feat: draft",
            ],
        );
        env.close_step(&agent.id).await;
        let on_tests = env.agent(&agent.id);
        assert_eq!(on_tests.step_id, "tests");
        assert_eq!(on_tests.steps[2].head_sha, Some(git(worktree, &["rev-parse", "HEAD"])));
    }

    #[tokio::test]
    async fn hands_the_ship_to_a_small_model_with_the_ticket_reference_from_the_qualification() {
        let env = Env::new();
        let agent = env.launch_default().await;
        env.walk_to(&agent.id, "ship").await;
        assert_eq!(env.terminals.last().arg("--model"), Some("haiku"));
        assert!(
            env.service
                .next_step(&agent.id)
                .expect("brief")
                .contains("ticket: ticket de qualification")
        );
    }

    #[tokio::test]
    async fn delivers_the_work_when_the_last_step_closes_with_the_merge_request() {
        let env = Env::new();
        let agent = env.launch_default().await;
        env.walk_to(&agent.id, "ship").await;
        let ship_session = env.terminals.last();
        assert_eq!(
            env.close_step(&agent.id).await,
            "Work delivered. This session is about to close; its id is kept for a resume."
        );
        assert!(ship_session.is_killed());
        let delivered = env.agent(&agent.id);
        assert_eq!((delivered.status, delivered.live_session_id), (AgentStatus::Done, None));
        assert_eq!(delivered.mr_url.as_deref(), Some("https://gitlab.example/mr/1"));
        let message = env.last_message();
        assert_eq!(message.text, "La MR de PROJ-224 est ouverte et attend votre relecture.");
        let open_mr = MessageAction::Link {
            label: "Ouvrir la MR".to_owned(),
            url: "https://gitlab.example/mr/1".to_owned(),
        };
        assert_eq!(message.actions, [open_mr, MessageAction::Archive]);
        assert_eq!(env.notifier.sent().last(), Some(&message.text));
        assert!(
            env.service
                .next_step(&agent.id)
                .expect("brief")
                .contains("no step left")
        );
        assert_eq!(
            refusal(env.service.complete_step(&agent.id, env.fields_for("ship", &[])).await),
            "le travail est déjà livré"
        );
    }

    #[tokio::test]
    async fn opens_no_session_for_an_agent_stopped_or_archived_while_its_next_one_was_due() {
        let env = Env::new();
        let stopped = env.launch_default().await;
        env.service
            .complete_step(&stopped.id, env.fields_for("qualification", &[]))
            .await
            .expect("complete");
        env.service
            .stop(&stopped.id, "le maître reprend la main")
            .expect("stop");
        let archived = env
            .launch(CreateAgentInput {
                ticket_ref: "PROJ-225".to_owned(),
                ..env.launch_input()
            })
            .await;
        env.service
            .complete_step(&archived.id, env.fields_for("qualification", &[]))
            .await
            .expect("complete");
        env.service.archive(&archived.id).expect("archive");
        settle().await;
        assert_eq!(env.terminals.count(), 2);
        assert!(!env.service.has_live_session(&stopped.id) && !env.service.has_live_session(&archived.id));
        assert_eq!(env.agent(&stopped.id).status, AgentStatus::Blocked);
    }

    #[tokio::test]
    async fn closes_a_step_once_when_the_agent_calls_twice() {
        let env = Env::new();
        let mut wakeups = env.wakeups();
        let agent = env.launch_default().await;
        let close = |env: &Env| {
            let (service, agent_id, input) = (
                env.service.clone(),
                agent.id.clone(),
                env.fields_for("qualification", &[]),
            );
            tokio::spawn(async move { service.complete_step(&agent_id, input).await })
        };
        // The first call waits for the orchestrator; the second one is refused while it does.
        let first = close(&env);
        wakeups.recv().await.expect("approval request");
        assert_eq!(
            refusal(close(&env).await.expect("join")),
            "cette étape attend déjà la décision de l'orchestrateur"
        );
        env.service
            .approve_step(&agent.id, "Parcours inchangé.", None)
            .expect("approve");
        first.await.expect("join").expect("reply");
        assert_eq!(env.path(&agent.id)[..2], ["qualification:done", "plan:current"]);
    }

    #[tokio::test]
    async fn closes_a_step_once_when_two_calls_wait_on_git_together() {
        let env = Env::new();
        let agent = env.launch_with(&["rapport", "tache", "rapport"]).await;
        let close = |env: &Env| {
            let (service, agent_id, input) = (env.service.clone(), agent.id.clone(), env.fields_for("rapport", &[]));
            tokio::spawn(async move { service.complete_step(&agent_id, input).await })
        };
        let (first, second) = (close(&env), close(&env));
        let outcomes = [first.await.expect("join"), second.await.expect("join")];
        let refusals: Vec<String> = outcomes
            .into_iter()
            .filter_map(std::result::Result::err)
            .map(|error| error.to_string())
            .collect();
        assert_eq!(
            refusals,
            ["l'étape Report est déjà close : appelle next_step pour la suivante"]
        );
        assert_eq!(
            env.path(&agent.id),
            ["rapport:done", "tache:current", "rapport:upcoming"]
        );
    }

    #[tokio::test]
    async fn blocks_the_agent_and_offers_to_resume_when_the_next_session_cannot_open() {
        let env = Env::new();
        let agent = env.launch_default().await;
        *env.terminals.failure.lock().expect("failure") = Some("lancement de claude impossible".to_owned());
        env.close_step(&agent.id).await;
        let blocked = env.agent(&agent.id);
        assert_eq!(
            (blocked.status, blocked.status_reason.as_deref()),
            (AgentStatus::Blocked, Some("lancement de claude impossible"))
        );
        let message = env.last_message();
        assert_eq!(
            message.text,
            "La session de PROJ-224 n'a pas pu s'ouvrir : lancement de claude impossible"
        );
        assert_eq!(message.actions, [MessageAction::Resume]);
        env.service.resume(&agent.id, None).expect("resume");
        assert_eq!(env.agent(&agent.id).status, AgentStatus::Working);
    }

    #[tokio::test]
    async fn keeps_the_agent_blocked_when_resuming_it_cannot_open_a_session_either() {
        let env = Env::new();
        let agent = env.launch_default().await;
        env.service.stop(&agent.id, "pause").expect("stop");
        *env.terminals.failure.lock().expect("failure") = Some("lancement de claude impossible".to_owned());
        assert_eq!(
            refusal(env.service.resume(&agent.id, None)),
            "lancement de claude impossible"
        );
        let blocked = env.agent(&agent.id);
        assert_eq!(
            (blocked.status, blocked.status_reason.as_deref()),
            (AgentStatus::Blocked, Some("lancement de claude impossible"))
        );
        assert!(blocked.waiting_since.is_some());
    }
}

mod validation_by_the_orchestrator {
    use super::*;

    /// Launches an agent and has it hand over its qualification, which waits for the orchestrator.
    async fn handed_over(
        env: &Env,
    ) -> (
        AgentRecord,
        mpsc::UnboundedReceiver<Wakeup>,
        tokio::task::JoinHandle<Result<String>>,
    ) {
        let mut wakeups = env.wakeups();
        let agent = env.launch_default().await;
        let (service, agent_id, input) = (
            env.service.clone(),
            agent.id.clone(),
            env.fields_for("qualification", &[]),
        );
        let closing = tokio::spawn(async move { service.complete_step(&agent_id, input).await });
        let request = wakeups.recv().await.expect("approval request");
        assert_eq!(
            request,
            Wakeup::Approval(ApprovalRequest {
                id: request_id(&request),
                agent_id: agent.id.clone(),
                step_id: agent.steps[0].id.clone()
            })
        );
        (agent, wakeups, closing)
    }

    #[tokio::test]
    async fn waits_for_the_decision_inside_the_tool_call_then_follows_the_workflow_it_set() {
        let env = Env::new();
        let (agent, _wakeups, closing) = handed_over(&env).await;
        // The fields are readable while the step waits: the orchestrator decides on them.
        assert_eq!(
            env.agent(&agent.id).steps[0].output.as_ref().expect("output")["size"],
            "size de qualification"
        );
        assert!(!closing.is_finished());
        let remaining = ids(&["implementation", "tests", "review", "correction", "ship"]);
        env.service
            .approve_step(&agent.id, "Plan retiré : petit correctif.", Some(&remaining))
            .expect("approve");
        let reply = closing.await.expect("join").expect("reply");
        assert!(reply.contains("The step Implementation goes to another session"));
        assert_eq!(
            env.path(&agent.id)[..3],
            ["qualification:done", "plan:skipped", "implementation:current"]
        );
        let decision = env.last_message();
        assert_eq!(
            (decision.kind, decision.text.as_str()),
            (MessageKind::Decision, "Plan retiré : petit correctif.")
        );
    }

    #[tokio::test]
    async fn keeps_the_agent_on_its_step_when_the_orchestrator_sends_it_back() {
        let env = Env::new();
        let (agent, _wakeups, closing) = handed_over(&env).await;
        env.service
            .reject_step(&agent.id, "critères manquants")
            .expect("reject");
        let message = refusal(closing.await.expect("join"));
        assert_eq!(
            message,
            "l'orchestrateur renvoie l'étape Qualification : critères manquants. Complète-la, puis rappelle complete_step."
        );
        assert_eq!(env.agent(&agent.id).step_id, "qualification");
        assert_eq!(env.last_message().text, "Renvoyée à l'agent : critères manquants");
        assert_eq!(
            refusal(env.service.reject_step(&agent.id, "encore")),
            "aucune étape de cet agent n'attend de validation"
        );
    }

    #[tokio::test]
    async fn refuses_a_decision_that_breaks_the_workflow_and_leaves_the_step_waiting() {
        let env = Env::new();
        let (agent, _wakeups, closing) = handed_over(&env).await;
        let message = refusal(
            env.service
                .approve_step(&agent.id, "Livrer tout de suite.", Some(&ids(&["ship"]))),
        );
        assert_eq!(message, "parcours refusé : ship doit venir après review");
        assert!(env.service.has_pending_approval(&agent.id) && !closing.is_finished());
        env.service
            .approve_step(&agent.id, "Parcours inchangé.", None)
            .expect("approve");
        closing.await.expect("join").expect("reply");
        assert_eq!(env.agent(&agent.id).step_id, "plan");
    }

    #[tokio::test]
    async fn approves_the_step_as_it_stands_when_the_orchestrator_never_answers() {
        let env = Env::new();
        let (agent, _wakeups, closing) = handed_over(&env).await;
        // The configured timeout of the tests is one second.
        closing.await.expect("join").expect("reply");
        assert_eq!(env.agent(&agent.id).step_id, "plan");
        let decisions: Vec<String> = env
            .messages()
            .into_iter()
            .filter(|message| message.kind == MessageKind::Decision)
            .map(|message| message.text)
            .collect();
        assert_eq!(
            decisions,
            ["Orchestrateur sans décision : étape validée, parcours inchangé."]
        );
    }

    #[tokio::test]
    async fn leaves_a_later_request_alone_when_an_earlier_one_is_settled() {
        let env = Env::new();
        let mut wakeups = env.wakeups();
        let agent = env.launch_default().await;
        let hand_over = |env: &Env| {
            let (service, agent_id, input) = (
                env.service.clone(),
                agent.id.clone(),
                env.fields_for("qualification", &[]),
            );
            tokio::spawn(async move { service.complete_step(&agent_id, input).await })
        };
        let first = hand_over(&env);
        let earlier = request_id(&wakeups.recv().await.expect("first request"));
        env.service.reject_step(&agent.id, "incomplet").expect("reject");
        first.await.expect("join").expect_err("sent back");
        let second = hand_over(&env);
        let later = request_id(&wakeups.recv().await.expect("second request"));
        // The run that rejected the first request ends after the agent handed its step over again.
        env.service.settle_approval(&agent.id, earlier).expect("settle");
        assert!(env.service.has_pending_approval(&agent.id));
        env.service.settle_approval(&agent.id, later).expect("settle");
        second.await.expect("join").expect("reply");
        assert_eq!(env.path(&agent.id)[..2], ["qualification:done", "plan:current"]);
    }

    #[tokio::test]
    async fn forgets_a_request_whose_agent_hung_up_before_the_decision() {
        let env = Env::new();
        let (agent, _wakeups, closing) = handed_over(&env).await;
        closing.abort();
        closing.await.expect_err("cancelled");
        assert!(!env.service.has_pending_approval(&agent.id));
        assert_eq!(
            refusal(env.service.approve_step(&agent.id, "Parcours inchangé.", None)),
            "aucune étape de cet agent n'attend de validation"
        );
        assert_eq!(env.agent(&agent.id).step_id, "qualification");
    }

    #[tokio::test]
    async fn approves_at_once_when_nobody_supervises() {
        let env = Env::new();
        let agent = env.launch_default().await;
        env.close_step(&agent.id).await;
        assert_eq!(env.agent(&agent.id).step_id, "plan");
    }
}

mod review_passes {
    use super::*;

    async fn at_review(env: &Env) -> AgentRecord {
        let agent = env.launch_default().await;
        env.walk_to(&agent.id, "review").await;
        agent
    }

    #[tokio::test]
    async fn drops_the_planned_correction_when_the_first_pass_finds_nothing() {
        let env = Env::new();
        let agent = at_review(&env).await;
        env.close_review_step(&agent.id, 0).await;
        assert_eq!(
            env.path(&agent.id)[4..],
            ["review:done", "correction:skipped", "ship:current"]
        );
    }

    #[tokio::test]
    async fn drops_the_planned_recheck_with_the_correction_when_the_first_pass_finds_nothing() {
        let env = Env::new();
        let agent = env
            .launch_with(&[
                "qualification",
                "implementation",
                "review",
                "correction",
                "review",
                "ship",
            ])
            .await;
        env.walk_to(&agent.id, "review").await;
        env.close_review_step(&agent.id, 0).await;
        assert_eq!(
            env.path(&agent.id)[2..],
            ["review:done", "correction:skipped", "review:skipped", "ship:current"]
        );
    }

    #[tokio::test]
    async fn demands_the_number_of_findings() {
        let env = Env::new();
        let agent = at_review(&env).await;
        let message = refusal(
            env.service
                .complete_step(&agent.id, env.fields_for("review", &[]))
                .await,
        );
        assert_eq!(
            message,
            "indique findingsAboveThreshold : le nombre de findings au seuil trouvés pendant cette passe"
        );
    }

    #[tokio::test]
    async fn alternates_corrections_and_passes_on_the_fix_commits_each_in_a_fresh_session() {
        let env = Env::new();
        let agent = at_review(&env).await;
        let first_pass = env.terminals.last();
        assert_eq!(first_pass.arg("--model"), Some("opus"));
        assert!(first_pass.prompt().starts_with(&format!(
            "/deep-review the branch {DEFAULT_BRANCH} of the repository {}",
            agent.worktree_path
        )));
        assert!(
            !first_pass
                .arg("--disallowedTools")
                .expect("withheld")
                .split(',')
                .any(|tool| tool == "Agent")
        );
        // deep-review and the minos subagents it launches come with the session, from ~/.atlas.
        let plugin = std::path::PathBuf::from(first_pass.arg("--plugin-dir").expect("plugin"));
        assert!(plugin.join("skills/deep-review/SKILL.md").is_file());
        assert!(plugin.join("agents/minos.md").is_file());

        env.close_review_step(&agent.id, 2).await;
        assert_eq!(
            env.path(&agent.id)[4..],
            [
                "review:done",
                "correction:current",
                "review#2:upcoming",
                "ship:upcoming"
            ]
        );
        assert_eq!(env.agent(&agent.id).steps[4].findings, Some(2));
        // The correction starts from the agent's file, not from the long build session.
        assert_eq!(env.terminals.last().arg("--resume"), None);

        env.close_step(&agent.id).await;
        let second_pass = env.terminals.last();
        let head = git(&env.repo, &["rev-parse", "HEAD"]);
        assert_eq!(
            (second_pass.arg("--model"), second_pass.arg("--resume")),
            (Some("sonnet"), None)
        );
        assert!(
            second_pass.prompt().contains(&format!("the commits {head}..HEAD"))
                && second_pass.prompt().contains("Pass 2:")
        );
        assert!(
            env.service
                .next_step(&agent.id)
                .expect("brief")
                .contains("Review pass 2 of 3")
        );

        env.close_review_step(&agent.id, 0).await;
        assert_eq!(
            env.path(&agent.id)[4..],
            ["review:done", "correction:done", "review#2:done", "ship:current"]
        );
    }

    #[tokio::test]
    async fn fixes_the_findings_before_a_ship_even_when_no_correction_was_planned() {
        let env = Env::new();
        let agent = env
            .launch_with(&["qualification", "implementation", "review", "ship"])
            .await;
        env.walk_to(&agent.id, "review").await;
        env.close_review_step(&agent.id, 1).await;
        assert_eq!(
            env.path(&agent.id)[2..],
            [
                "review:done",
                "correction:current",
                "review#2:upcoming",
                "ship:upcoming"
            ]
        );
    }

    #[tokio::test]
    async fn only_reports_when_the_review_is_the_whole_job() {
        let env = Env::new();
        let agent = env.launch_with(&["review"]).await;
        assert!(
            env.terminals
                .last()
                .prompt()
                .starts_with("/deep-review the commit HEAD of the repository")
        );
        env.close_review_step(&agent.id, 3).await;
        assert_eq!(env.agent(&agent.id).status, AgentStatus::Done);
        assert_eq!(
            env.last_message().text,
            "PROJ-224 est terminé et attend votre relecture."
        );
        assert_eq!(
            env.last_message().actions,
            [MessageAction::Session, MessageAction::Archive]
        );
    }

    #[tokio::test]
    async fn stops_for_the_master_after_three_passes_that_still_find_something() {
        let env = Env::new();
        let agent = at_review(&env).await;
        for _ in 0..2 {
            env.close_review_step(&agent.id, 1).await;
            env.close_step(&agent.id).await;
        }
        let sessions_before = env.terminals.count();
        let reply = env.close_review_step(&agent.id, 2).await;
        assert!(reply.starts_with("Step closed. The master is told"));
        let held = env.agent(&agent.id);
        let reason = "3 passes de review sans converger : il reste 2 findings au seuil";
        assert_eq!(
            (held.status, held.status_reason.as_deref(), held.step_id.as_str()),
            (AgentStatus::Blocked, Some(reason), "ship")
        );
        assert_eq!((held.live_session_id, env.terminals.count()), (None, sessions_before));
        assert_eq!(
            env.last_message().text,
            format!("PROJ-224 : {reason}. Que voulez-vous faire ?")
        );

        env.service.resume(&agent.id, None).expect("resume");
        assert_eq!(env.agent(&agent.id).status, AgentStatus::Working);
        assert_eq!(env.terminals.last().arg("--model"), Some("haiku"));
    }
}

mod stopping_an_agent_and_moving_it_on {
    use super::*;

    #[tokio::test]
    async fn stops_an_agent_without_taking_it_off_the_board_and_resumes_it() {
        let env = Env::new();
        let agent = env.launch_default().await;
        let session = env.terminals.last();
        env.service.stop(&agent.id, "le maître reprend la main").expect("stop");
        let stopped = env.agent(&agent.id);
        assert_eq!(
            (stopped.status, stopped.status_reason.as_deref()),
            (AgentStatus::Blocked, Some("arrêté : le maître reprend la main"))
        );
        assert_eq!((stopped.live_session_id, stopped.archived_at), (None, None));
        assert!(session.is_killed());

        env.clock.advance_minutes(5);
        env.service.resume(&agent.id, None).expect("resume");
        let resumed = env.agent(&agent.id);
        assert_eq!(
            (resumed.status, resumed.status_reason, resumed.step_wait_ms),
            (AgentStatus::Working, None, 5 * 60_000)
        );
        assert_eq!(env.terminals.last().arg("--resume"), agent.live_session_id.as_deref());
        assert_eq!(env.terminals.last().prompt(), RESUME_PROMPT);
        assert_eq!(
            refusal(env.service.resume(&agent.id, None)),
            "l'agent PROJ-224 a déjà une session ouverte"
        );
    }

    #[tokio::test]
    async fn gives_up_the_current_review_and_ships_in_a_fresh_session() {
        let env = Env::new();
        let agent = env.launch_default().await;
        env.walk_to(&agent.id, "review").await;
        env.close_review_step(&agent.id, 1).await;
        env.close_step(&agent.id).await;
        assert_eq!(env.path(&agent.id)[6..], ["review#2:current", "ship:upcoming"]);

        assert!(refusal(env.service.jump_to(&agent.id, &ids(&["inconnue"]), "x")).starts_with("parcours refusé"));
        assert_eq!(env.agent(&agent.id).step_id, "review");
        let review_session = env.terminals.last();
        env.service
            .jump_to(&agent.id, &ids(&["ship"]), "Le maître livre sans attendre la review 2.")
            .expect("jump");
        assert!(review_session.is_killed());
        assert_eq!(
            env.path(&agent.id)[5..],
            ["correction:done", "review#2:skipped", "ship:current"]
        );
        let jumped = env.agent(&agent.id);
        assert_eq!((jumped.status, jumped.step_id.as_str()), (AgentStatus::Working, "ship"));
        assert_eq!(env.terminals.last().arg("--model"), Some("haiku"));
        let message = env.last_message();
        assert_eq!(
            (message.kind, message.text.as_str()),
            (MessageKind::Workflow, "Le maître livre sans attendre la review 2.")
        );
    }
}

mod changing_a_workflow_in_flight {
    use super::*;

    #[tokio::test]
    async fn replaces_what_has_not_started_keeps_the_history_and_shows_the_change_in_the_chat() {
        let env = Env::new();
        let agent = env.launch_default().await;
        env.service
            .set_workflow(
                &agent.id,
                &ids(&["implementation", "tests", "review", "ship"]),
                "Petit correctif : sans plan.",
            )
            .expect("set");
        assert_eq!(
            env.path(&agent.id),
            [
                "qualification:current",
                "plan:skipped",
                "correction:skipped",
                "implementation:upcoming",
                "tests:upcoming",
                "review:upcoming",
                "ship:upcoming"
            ]
        );
        let message = env.last_message();
        assert_eq!(
            (message.kind, message.text.as_str()),
            (MessageKind::Workflow, "Petit correctif : sans plan.")
        );
        assert_eq!(env.terminals.count(), 1);
        assert_eq!(
            refusal(env.service.set_workflow(&agent.id, &ids(&["tests"]), "x")),
            "parcours refusé : tests doit venir après implementation ou correction"
        );
    }

    #[tokio::test]
    async fn survives_a_change_made_while_the_next_step_is_waiting_for_its_session() {
        let env = Env::new();
        let agent = env.launch_default().await;
        env.service
            .complete_step(&agent.id, env.fields_for("qualification", &[]))
            .await
            .expect("complete");
        env.service
            .set_workflow(
                &agent.id,
                &ids(&["implementation", "tests", "review", "ship"]),
                "Sans plan.",
            )
            .expect("set");
        settle().await;
        let changed = env.agent(&agent.id);
        assert_eq!(
            (changed.status, changed.step_id.as_str()),
            (AgentStatus::Working, "implementation")
        );
        assert_eq!(env.terminals.count(), 2);
        assert!(env.messages().iter().all(|message| message.kind != MessageKind::Text));
    }

    #[tokio::test]
    async fn gives_new_steps_to_a_delivered_agent_and_reopens_it() {
        let env = Env::new();
        git(&env.repo, &["branch", "fix/PROJ-224/mrBranch"]);
        let input = CreateAgentInput {
            steps: Some(ids(&["review-mr"])),
            mr_url: Some("https://gitlab.example/mr/9".to_owned()),
            branch: Some("fix/PROJ-224/mrBranch".to_owned()),
            ..env.launch_input()
        };
        let agent = env.launch(input).await;
        env.close_step(&agent.id).await;
        assert_eq!(env.agent(&agent.id).status, AgentStatus::Done);
        // A merge request the agent was given to read is not one it opened.
        assert_eq!(
            env.last_message().text,
            "PROJ-224 est terminé et attend votre relecture."
        );
        let steps = ids(&["correction", "tests", "review", "ship"]);
        env.service
            .set_workflow(&agent.id, &steps, "Le maître demande de corriger les retours.")
            .expect("set");
        let reopened = env.agent(&agent.id);
        assert_eq!(
            (reopened.status, reopened.step_id.as_str()),
            (AgentStatus::Working, "correction")
        );
        assert_eq!(env.terminals.last().arg("--model"), Some("sonnet"));
    }
}

mod waiting_for_the_master {
    use super::*;

    #[tokio::test]
    async fn warns_the_master_without_relaying_the_question_once_and_resumes_on_the_next_prompt() {
        let env = Env::new();
        let agent = env.launch_default().await;
        let reply = env.service.ask_user(&agent.id, "Quel délai de relance ?").expect("ask");
        assert!(reply.starts_with("The master is told, but he does not see the text passed to ask_user"));
        let waiting = env.agent(&agent.id);
        assert_eq!(
            (waiting.status, waiting.status_reason.as_deref()),
            (AgentStatus::Waiting, Some("Quel délai de relance ?"))
        );
        assert_eq!(waiting.waiting_since.as_deref(), Some("2026-01-01T10:00:00.000Z"));
        assert_eq!(
            env.last_message().text,
            "L'agent qui travaille sur PROJ-224 a besoin de votre aide."
        );
        let messages_before = env.messages().len();

        env.clock.advance_minutes(2);
        env.service.ask_user(&agent.id, "Autre question").expect("ask again");
        assert_eq!(env.messages().len(), messages_before);
        assert_eq!(
            env.agent(&agent.id).waiting_since.as_deref(),
            Some("2026-01-01T10:00:00.000Z")
        );

        env.clock.advance_minutes(3);
        env.service.on_user_prompt(&agent.id, Some("7 jours")).expect("answer");
        let resumed = env.agent(&agent.id);
        assert_eq!(
            (resumed.status, resumed.status_reason, resumed.waiting_since),
            (AgentStatus::Working, None, None)
        );
        assert_eq!(resumed.step_wait_ms, 5 * 60_000);
    }

    #[tokio::test]
    async fn does_not_mistake_the_prompt_atlas_starts_a_session_with_for_the_master_answering() {
        let env = Env::new();
        let agent = env.launch_default().await;
        env.service.ask_user(&agent.id, "Question").expect("ask");
        let launch_prompt = env.terminals.last().prompt().to_owned();
        env.service
            .on_user_prompt(&agent.id, Some(&format!("  {launch_prompt}\n")))
            .expect("atlas prompt");
        assert_eq!(env.agent(&agent.id).status, AgentStatus::Waiting);
        // Typed again, it is the master's own.
        env.service
            .on_user_prompt(&agent.id, Some(&launch_prompt))
            .expect("master prompt");
        assert_eq!(env.agent(&agent.id).status, AgentStatus::Working);
    }

    #[tokio::test]
    async fn refuses_to_close_a_step_while_the_master_has_not_answered() {
        let env = Env::new();
        let agent = env.launch_default().await;
        env.service.ask_user(&agent.id, "Question").expect("ask");
        let message = refusal(
            env.service
                .complete_step(&agent.id, env.fields_for("qualification", &[]))
                .await,
        );
        assert!(message.starts_with("tu as posé une question au maître avec ask_user"));
    }

    #[tokio::test]
    async fn marks_a_declared_block_and_warns_the_master() {
        let env = Env::new();
        let agent = env.launch_default().await;
        env.service.block(&agent.id, "accès refusé au registre").expect("block");
        let blocked = env.agent(&agent.id);
        assert_eq!(
            (blocked.status, blocked.status_reason.as_deref()),
            (AgentStatus::Blocked, Some("accès refusé au registre"))
        );
        assert_eq!(
            env.last_message().text,
            "L'agent qui travaille sur PROJ-224 est bloqué et a besoin de votre aide."
        );
        assert_eq!(env.notifier.sent().len(), 1);
    }

    #[tokio::test]
    async fn refuses_to_block_an_agent_whose_work_is_delivered() {
        let env = Env::new();
        let agent = env.launch_with(&["rapport"]).await;
        env.close_step(&agent.id).await;
        assert_eq!(
            refusal(env.service.block(&agent.id, "trop tard")),
            "le travail est déjà livré"
        );
        assert_eq!(env.agent(&agent.id).status, AgentStatus::Done);
    }
}

mod the_stop_hook {
    use super::*;

    #[tokio::test]
    async fn sends_a_working_agent_back_to_its_step_a_limited_number_of_times() {
        let env = Env::new();
        let agent = env.launch_default().await;
        for _ in 0..3 {
            assert!(
                env.service
                    .on_stop(&agent.id)
                    .expect("stop")
                    .expect("refused")
                    .starts_with("Ton étape Atlas n'est pas close")
            );
        }
        assert_eq!(env.service.on_stop(&agent.id).expect("stop"), None);
        // Any sign of progress gives the agent its chances back.
        env.service.next_step(&agent.id).expect("brief");
        assert!(env.service.on_stop(&agent.id).expect("stop").is_some());
    }

    #[tokio::test]
    async fn lets_a_waiting_agent_stop() {
        let env = Env::new();
        let agent = env.launch_default().await;
        env.service.ask_user(&agent.id, "Question").expect("ask");
        assert_eq!(env.service.on_stop(&agent.id).expect("stop"), None);
    }
}

mod sessions {
    use super::*;

    #[tokio::test]
    async fn restarts_the_current_step_in_a_fresh_session() {
        let env = Env::new();
        let agent = env.launch_default().await;
        let first = env.terminals.last();
        env.service.restart(&agent.id).expect("restart");
        assert!(first.is_killed());
        let fresh = env.terminals.last();
        assert_eq!(fresh.arg("--resume"), None);
        assert_ne!(fresh.arg("--session-id"), first.arg("--session-id"));
        assert!(
            fresh
                .prompt()
                .starts_with("You are an Atlas agent in charge of PROJ-224")
        );
        assert_eq!(
            env.agent(&agent.id).live_session_id.as_deref(),
            fresh.arg("--session-id")
        );
        // The end of the session Atlas closed does not take the new one down with it.
        tick().await;
        assert!(env.service.has_live_session(&agent.id));
    }

    #[tokio::test]
    async fn reopens_the_last_session_of_a_delivered_agent() {
        let env = Env::new();
        let agent = env.launch_with(&["tache"]).await;
        env.close_step(&agent.id).await;
        env.service
            .resume(&agent.id, Some("Une question du maître.".to_owned()))
            .expect("resume");
        let reopened = env.terminals.last();
        assert_eq!(reopened.arg("--resume"), agent.live_session_id.as_deref());
        assert_eq!(reopened.prompt(), "Une question du maître.");
        assert_eq!(
            refusal(env.service.restart(&agent.id)),
            "PROJ-224 est livré : rouvre sa session avec resume_agent"
        );
    }

    #[tokio::test]
    async fn shows_the_orchestrator_the_command_line_and_the_screen_of_an_open_session() {
        let env = Env::new();
        let agent = env.launch_default().await;
        let session = env.terminals.last();
        session.print("ligne 1\r\nlig");
        session.print("ne 2 \u{1b}[32mverte\u{1b}[0m\r\n");
        tick().await;
        let (command_line, screen) = env.service.read_terminal(&agent.id, 10).expect("screen");
        assert!(
            command_line.starts_with("claude --session-id ")
                && command_line.contains(" -- 'You are an Atlas agent in charge of PROJ-224")
        );
        assert_eq!(screen, "ligne 1\nligne 2 verte");

        env.service.close(&agent.id).expect("close");
        assert_eq!(
            refusal(env.service.read_terminal(&agent.id, 10)),
            "PROJ-224 n'a pas de session ouverte : resume_agent la rouvre"
        );
    }

    #[tokio::test]
    async fn tells_the_clients_what_a_session_prints_whole_characters_only_then_its_end() {
        let env = Env::new();
        let agent = env.launch_default().await;
        let mut events = env.service.subscribe_terminal();
        let session = env.terminals.last();
        let accent = "é".as_bytes();
        session.print("caf");
        // A character cut between two reads waits for its end.
        session.print_bytes(&accent[..1]);
        session.print_bytes(&accent[1..]);
        session.exit(Some(2));
        tick().await;
        let output = |data: &str| TerminalEvent::Output {
            agent_id: agent.id.clone(),
            data: data.to_owned(),
        };
        assert_eq!(events.recv().await.expect("output"), output("caf"));
        assert_eq!(events.recv().await.expect("output"), output("é"));
        assert_eq!(
            events.recv().await.expect("exit"),
            TerminalEvent::Exit {
                agent_id: agent.id.clone(),
                code: Some(2)
            }
        );
    }

    #[tokio::test]
    async fn redraws_the_screen_for_a_terminal_opened_later() {
        let env = Env::new();
        let agent = env.launch_default().await;
        env.terminals.last().print("déjà affiché\r\n");
        tick().await;
        let (screen, mut output) = env.service.attach_terminal(&agent.id).expect("attached");
        assert!(screen.contains("déjà affiché"));
        env.terminals.last().print("suite");
        tick().await;
        assert_eq!(
            output.recv().await.expect("output"),
            TerminalEvent::Output {
                agent_id: agent.id.clone(),
                data: "suite".to_owned()
            }
        );
        env.service.close(&agent.id).expect("close");
        assert!(env.service.attach_terminal(&agent.id).is_none());
    }

    #[tokio::test]
    async fn types_a_message_into_the_session_which_is_not_taken_for_the_master_answering() {
        let env = Env::new();
        let agent = env.launch_default().await;
        env.service.ask_user(&agent.id, "Question").expect("ask");
        env.service
            .send_to_agent(&agent.id, "Le délai est de\n7 jours.")
            .expect("send");
        let session = env.terminals.last();
        assert_eq!(session.written(), "\u{1b}[200~Le délai est de\n7 jours.\u{1b}[201~");
        tokio::time::sleep(PASTE_SUBMIT_DELAY + Duration::from_millis(100)).await;
        assert!(session.written().ends_with("\u{1b}[201~\r"));
        env.service
            .on_user_prompt(&agent.id, Some("Le délai est de\n7 jours."))
            .expect("prompt");
        assert_eq!(env.agent(&agent.id).status, AgentStatus::Waiting);

        // Without an open session, the session reopens with the message as its prompt.
        env.service.close(&agent.id).expect("close");
        env.service.send_to_agent(&agent.id, "Reprends.").expect("send");
        assert_eq!((env.terminals.count(), env.terminals.last().prompt()), (2, "Reprends."));
    }

    #[tokio::test]
    async fn reports_once_a_session_that_waits_for_input_while_the_agent_should_work() {
        let env = Env::new();
        let mut wakeups = env.wakeups();
        let agent = env.launch_default().await;
        let idle = Wakeup::Anomaly(Anomaly::Idle {
            agent_id: agent.id.clone(),
        });
        env.service.on_idle(&agent.id).expect("idle");
        env.service.on_idle(&agent.id).expect("idle again");
        assert_eq!(wakeups.try_recv().ok(), Some(idle.clone()));
        assert!(wakeups.try_recv().is_err());
        // A tool call shows the agent at work again: the next silence is a new one.
        env.service.on_post_tool_use(&agent.id).expect("tool");
        env.service.on_idle(&agent.id).expect("idle");
        assert_eq!(wakeups.try_recv().ok(), Some(idle));
        // Waiting for the master is the expected kind of silence.
        env.service.on_post_tool_use(&agent.id).expect("tool");
        env.service.ask_user(&agent.id, "Question").expect("ask");
        env.service.on_idle(&agent.id).expect("idle");
        assert!(wakeups.try_recv().is_err());
    }

    #[tokio::test]
    async fn reports_a_session_that_dies_while_the_agent_works() {
        let env = Env::new();
        let mut wakeups = env.wakeups();
        let agent = env.launch_default().await;
        env.terminals.last().exit(Some(1));
        tick().await;
        assert_eq!(
            wakeups.try_recv().ok(),
            Some(Wakeup::Anomaly(Anomaly::SessionDied {
                agent_id: agent.id.clone(),
                exit_code: Some(1)
            }))
        );
        assert_eq!(env.agent(&agent.id).live_session_id, None);
        assert!(!env.service.has_live_session(&agent.id));
        let session = env.store.list_sessions(&agent.id).expect("sessions").remove(0);
        assert_eq!(session.outcome.as_deref(), Some("exited with code 1"));
    }

    #[tokio::test]
    async fn closes_what_the_store_believes_live_after_a_daemon_restart() {
        let env = Env::new();
        let agent = env.launch_default().await;
        let waiting = env
            .launch(CreateAgentInput {
                ticket_ref: "PROJ-225".to_owned(),
                ..env.launch_input()
            })
            .await;
        env.service.ask_user(&waiting.id, "Quelle base ?").expect("ask");
        env.service.recover_after_restart().expect("recover");
        let stopped = env.agent(&agent.id);
        assert_eq!(stopped.live_session_id, None);
        assert_eq!(stopped.status, AgentStatus::Blocked);
        assert!(stopped.status_reason.expect("reason").starts_with("Atlas a redémarré"));
        let session = env.store.list_sessions(&agent.id).expect("sessions").remove(0);
        assert_eq!(session.outcome.as_deref(), Some("daemon restarted"));
        // An agent that waited for the master still does: his answer reopens its session.
        assert_eq!(env.agent(&waiting.id).status, AgentStatus::Waiting);
    }

    #[tokio::test]
    async fn shows_stopped_an_agent_a_clean_stop_left_at_work() {
        let env = Env::new();
        let agent = env.launch_default().await;
        env.service.shutdown();
        env.service.recover_after_restart().expect("recover");
        assert_eq!(env.agent(&agent.id).status, AgentStatus::Blocked);
        let resumed = env.service.resume(&agent.id, None).expect("resume");
        assert_eq!((resumed.status, resumed.status_reason), (AgentStatus::Working, None));
    }

    #[tokio::test]
    async fn closes_every_session_when_the_daemon_stops() {
        let env = Env::new();
        let agent = env.launch_default().await;
        env.service.shutdown();
        assert!(env.terminals.last().is_killed());
        assert_eq!(
            env.store.list_sessions(&agent.id).expect("sessions")[0]
                .outcome
                .as_deref(),
            Some("daemon stopped")
        );
    }
}

mod work_that_spans_two_repositories {
    use super::*;

    /// A second declared project, `web`, next to the one agents are launched in.
    async fn with_second_project(env: &Env) -> String {
        let web = env.repo.parent().expect("repos").join("web-front");
        init_repository(&web);
        let input = RepoInput {
            name: "web".to_owned(),
            path: web.to_string_lossy().into_owned(),
            ..RepoInput::default()
        };
        env.service.projects().add(input).await.expect("project").path
    }

    #[tokio::test]
    async fn opens_a_folder_in_a_declared_project_on_the_same_branch_and_says_so_in_the_chat() {
        let env = Env::new();
        let web_path = with_second_project(&env).await;
        let agent = env.launch_default().await;
        // The folder name of the repository is enough to name the project.
        let reply = env
            .service
            .request_repo(&agent.id, "Web Front", "adapter le formulaire")
            .await
            .expect("reply");
        let workspace = env.agent(&agent.id).workspaces.remove(0);
        assert_eq!(
            (workspace.repo_name.as_str(), workspace.repo_path.as_str()),
            ("web", web_path.as_str())
        );
        assert_eq!(
            (workspace.branch.as_str(), workspace.base_branch.as_str()),
            (DEFAULT_BRANCH, "main")
        );
        assert!(reply.starts_with(&format!(
            "Folder ready: {}, branch {DEFAULT_BRANCH} (base main).",
            workspace.worktree_path
        )));
        let message = env.last_message();
        assert_eq!(
            (message.kind, message.text.as_str()),
            (
                MessageKind::Decision,
                "Dépôt web ouvert à l'agent, à sa demande : adapter le formulaire"
            )
        );
        assert_eq!(
            env.service
                .request_repo(&agent.id, "web", "encore")
                .await
                .expect("again"),
            format!("You already work in this project: {}.", workspace.worktree_path)
        );
    }

    #[tokio::test]
    async fn warns_the_master_about_a_project_that_is_not_declared() {
        let env = Env::new();
        let agent = env.launch_default().await;
        let message = refusal(env.service.request_repo(&agent.id, "billing", "ajouter un champ").await);
        assert!(message.starts_with("projet inconnu : billing. Projets déclarés : aucun."));
        assert!(
            env.last_message()
                .text
                .contains("a besoin d'un autre dépôt, billing, qui n'est pas un projet déclaré")
        );
    }

    #[tokio::test]
    async fn carries_the_second_folder_into_the_sessions_the_briefs_and_the_commit_check() {
        let env = Env::new();
        with_second_project(&env).await;
        let agent = env.launch_default().await;
        env.service
            .request_repo(&agent.id, "web", "adapter le formulaire")
            .await
            .expect("reply");
        let second = env.agent(&agent.id).workspaces.remove(0).worktree_path;
        assert!(
            env.service
                .next_step(&agent.id)
                .expect("brief")
                .contains(&format!("Other working folder (project web): {second}"))
        );
        env.walk_to(&agent.id, "implementation").await;
        assert_eq!(env.terminals.last().arg("--add-dir"), Some(second.as_str()));

        std::fs::write(Path::new(&second).join("draft.txt"), "work").expect("draft");
        let message = refusal(
            env.service
                .complete_step(&agent.id, env.fields_for("implementation", &[]))
                .await,
        );
        assert!(message.starts_with(&format!("git status n'est pas propre dans {second} :")));
    }
}

mod work_on_a_merge_request_whose_branch_nobody_named {
    use super::*;

    async fn on_a_merge_request(env: &Env) -> AgentRecord {
        git(&env.repo, &["branch", "fix/mr-branch"]);
        let input = CreateAgentInput {
            steps: Some(ids(&["qualification", "implementation"])),
            start_at: Some("implementation".to_owned()),
            mr_url: Some("https://gitlab.example/mr/9".to_owned()),
            ..env.launch_input()
        };
        env.launch(input).await
    }

    #[tokio::test]
    async fn starts_without_a_branch_and_tells_the_agent_to_find_the_one_of_the_merge_request() {
        let env = Env::new();
        let agent = on_a_merge_request(&env).await;
        assert_eq!(agent.branch, "");
        assert!(
            env.service
                .next_step(&agent.id)
                .expect("brief")
                .contains(NO_BRANCH_NOTICE)
        );
        assert!(
            env.service
                .on_session_start(&agent.id)
                .expect("brief")
                .contains("without a branch for now (base main)")
        );
        assert_eq!(
            refusal(
                env.service
                    .complete_step(&agent.id, env.fields_for("implementation", &[]))
                    .await
            ),
            NO_BRANCH_NOTICE
        );
        assert!(refusal(env.service.request_repo(&agent.id, "web", "x").await).starts_with("projet inconnu"));
    }

    #[tokio::test]
    async fn moves_the_agent_onto_the_branch_it_found_in_a_fresh_session_there_and_drops_the_folder_it_left() {
        let env = Env::new();
        let agent = on_a_merge_request(&env).await;
        let first_session = env.terminals.last();
        let reply = env
            .service
            .work_on_branch(&agent.id, "fix/mr-branch", Some("develop"))
            .await
            .expect("reply");
        let moved = env.agent(&agent.id);
        assert!(reply.starts_with(&format!(
            "Atlas settles you in {}, on fix/mr-branch.",
            moved.worktree_path
        )));
        assert_eq!(
            (moved.branch.as_str(), moved.base_branch.as_str(), moved.owns_worktree),
            ("fix/mr-branch", "develop", true)
        );
        assert_ne!(moved.worktree_path, agent.worktree_path);
        assert!(!Path::new(&agent.worktree_path).exists());
        assert_eq!(
            env.last_message().text,
            "L'agent s'installe sur la branche fix/mr-branch, qu'il a trouvée lui-même."
        );

        settle().await;
        assert!(first_session.is_killed());
        let fresh = env.terminals.last();
        assert_eq!(
            (fresh.request.cwd.as_str(), fresh.arg("--resume")),
            (moved.worktree_path.as_str(), None)
        );
        assert_eq!(
            env.service
                .work_on_branch(&agent.id, "fix/mr-branch", None)
                .await
                .expect("again"),
            format!("You are already on fix/mr-branch, in {}.", moved.worktree_path)
        );
    }

    #[tokio::test]
    async fn refuses_a_branch_that_does_not_exist_and_leaves_the_agent_where_it_is() {
        let env = Env::new();
        let agent = on_a_merge_request(&env).await;
        let message = refusal(env.service.work_on_branch(&agent.id, "missing/branch", None).await);
        assert_eq!(
            message,
            "impossible de s'installer sur missing/branch : la branche missing/branch n'existe ni dans le dépôt ni sur origin. work_on_branch ne prend qu'une branche qui existe déjà."
        );
        assert_eq!(env.agent(&agent.id).worktree_path, agent.worktree_path);
        assert!(env.service.has_live_session(&agent.id));
    }
}

mod archiving {
    use super::*;

    #[tokio::test]
    async fn takes_the_agent_off_the_board_closes_its_session_and_frees_its_folder_for_another_agent() {
        let env = Env::new();
        let mut events = env.service.subscribe();
        let in_place = || CreateAgentInput {
            in_place: Some(true),
            ..env.launch_input()
        };
        let agent = env.launch(in_place()).await;
        env.service.archive(&agent.id).expect("archive");
        assert!(env.terminals.last().is_killed());
        assert!(env.agent(&agent.id).archived_at.is_some());
        assert!(env.service.views().expect("views").is_empty());
        let mut archived = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let Event::Archived(agent_id) = event {
                archived.push(agent_id);
            }
        }
        assert_eq!(archived, std::slice::from_ref(&agent.id));
        // The folder is free again, and the branch and the folder were left as they were.
        env.launch(in_place()).await;
        assert!(env.repo.exists());
    }
}

mod supervision_and_broadcasts {
    use super::*;

    #[tokio::test]
    async fn flags_an_agent_past_the_threshold_of_its_brick_once_waiting_time_excluded() {
        let env = Env::new();
        let mut wakeups = env.wakeups();
        let agent = env.launch_default().await;
        let threshold = i64::from(env.catalogue()["qualification"].threshold_minutes);
        env.service.ask_user(&agent.id, "Question").expect("ask");
        env.clock.advance_minutes(threshold + 5);
        env.service.on_user_prompt(&agent.id, Some("réponse")).expect("answer");
        env.clock.advance_minutes(threshold - 1);
        assert!(env.service.supervise().expect("supervise").is_empty());

        env.clock.advance_minutes(2);
        let anomaly = Anomaly::Threshold {
            agent_id: agent.id.clone(),
        };
        assert_eq!(
            env.service.supervise().expect("supervise"),
            std::slice::from_ref(&anomaly)
        );
        assert_eq!(wakeups.try_recv().ok(), Some(Wakeup::Anomaly(anomaly)));
        assert!(env.agent(&agent.id).threshold_alerted);
        assert!(env.service.supervise().expect("supervise").is_empty());
        // A new step starts with its own threshold.
        env.close_step(&agent.id).await;
        assert!(!env.agent(&agent.id).threshold_alerted);
    }

    #[tokio::test]
    async fn hands_a_broadcast_to_an_agent_at_its_next_tool_call() {
        let env = Env::new();
        let agent = env.launch_default().await;
        assert_eq!(env.service.on_post_tool_use(&agent.id).expect("tool"), None);
        env.clock.advance_minutes(1);
        assert_eq!(
            env.service.broadcast("Ne poussez rien avant midi.").expect("broadcast"),
            1
        );
        assert_eq!(
            env.service.on_post_tool_use(&agent.id).expect("tool").as_deref(),
            Some("Message of the master to all the agents: Ne poussez rien avant midi.")
        );
        assert_eq!(env.service.on_post_tool_use(&agent.id).expect("tool"), None);
    }
}

mod installing_the_dependencies_of_a_new_worktree {
    use super::*;

    async fn with_setup(env: &Env, command: &str) {
        let input = RepoInput {
            name: "shop".to_owned(),
            path: env.repo_path(),
            setup: Some(Some(command.to_owned())),
            ..RepoInput::default()
        };
        env.service.projects().add(input).await.expect("project");
    }

    fn building(env: &Env) -> CreateAgentInput {
        CreateAgentInput {
            steps: Some(ids(&["qualification", "implementation"])),
            start_at: Some("implementation".to_owned()),
            ..env.launch_input()
        }
    }

    #[tokio::test]
    async fn installs_in_the_worktree_atlas_creates_and_makes_the_building_step_wait_for_it() {
        let env = Env::new();
        with_setup(&env, "sleep 0.4 && pwd").await;
        let agent = env.launch(building(&env)).await;
        // Asked at once, the brief says the install is still running.
        assert!(
            env.service
                .next_step(&agent.id)
                .expect("brief")
                .contains("Atlas is still installing the dependencies in")
        );
        let brief = env.service.next_step_when_ready(&agent.id).await.expect("brief");
        assert!(!brief.contains("dépendances"));
        let log = std::fs::read_to_string(env.home.join("agents").join(&agent.id).join("setup-1.log")).expect("log");
        assert_eq!(log.trim(), agent.worktree_path);
    }

    #[tokio::test]
    async fn tells_the_building_step_and_the_master_when_the_install_failed() {
        let env = Env::new();
        with_setup(&env, "exit 3").await;
        let agent = env.launch(building(&env)).await;
        let brief = env.service.next_step_when_ready(&agent.id).await.expect("brief");
        assert!(brief.contains(&format!(
            "Installing the dependencies failed in {} (exit 3)",
            agent.worktree_path
        )));
        tick().await;
        assert!(
            env.last_message()
                .text
                .starts_with("L'installation des dépendances de PROJ-224 a échoué (exit 3) : voir ")
        );
    }

    #[tokio::test]
    async fn leaves_a_folder_atlas_did_not_create_as_it_is() {
        let env = Env::new();
        with_setup(&env, "pwd").await;
        let agent = env
            .launch(CreateAgentInput {
                in_place: Some(true),
                ..building(&env)
            })
            .await;
        settle().await;
        assert!(!env.home.join("agents").join(&agent.id).join("setup-1.log").exists());
    }
}

mod the_memory_of_a_project {
    use super::*;

    #[tokio::test]
    async fn offers_a_note_to_the_master_who_decides_to_keep_it() {
        let env = Env::new();
        let agent = env.launch_default().await;
        assert_eq!(
            env.service
                .propose_note(&agent.id, "L'API tourne sur api.test", None, false)
                .expect("undeclared"),
            "This repository is not a project declared in Atlas: nothing to keep, go on with your work."
        );
        let input = RepoInput {
            name: "shop".to_owned(),
            path: env.repo_path(),
            ..RepoInput::default()
        };
        env.service.projects().add(input).await.expect("project");

        env.service
            .propose_note(&agent.id, " L'API tourne sur api.test ", None, false)
            .expect("proposed");
        let message = env.last_message();
        assert_eq!(
            message.text,
            "L'agent qui travaille sur PROJ-224 propose de retenir pour le projet shop : L'API tourne sur api.test"
        );
        let save = MessageAction::SaveNote {
            repo: Some("shop".to_owned()),
            note: "L'API tourne sur api.test".to_owned(),
        };
        assert_eq!(message.actions, [save]);
        // Nothing is kept until the master says so.
        assert!(
            env.service
                .projects()
                .require("shop")
                .expect("project")
                .notes
                .is_empty()
        );

        env.service
            .projects()
            .add_note("shop", "L'API tourne sur api.test")
            .expect("kept");
        assert_eq!(
            env.service
                .propose_note(&agent.id, "L'API tourne sur api.test", Some("shop"), false)
                .expect("known"),
            "This note is already in memory. Go on with your work."
        );
        env.service
            .propose_note(&agent.id, "Commits en anglais", None, true)
            .expect("for all");
        assert_eq!(
            env.last_message().actions,
            [MessageAction::SaveNote {
                repo: None,
                note: "Commits en anglais".to_owned()
            }]
        );
        assert_eq!(
            refusal(env.service.propose_note(&agent.id, "deux\nlignes", None, false)),
            "note refusée : une seule ligne, 300 caractères au plus"
        );
    }

    #[tokio::test]
    async fn changes_the_catalogue_only_once_the_master_validates_it() {
        let env = Env::new();
        let change = CatalogueChange::Disable {
            id: "rapport".to_owned(),
        };
        assert_eq!(
            refusal(env.service.propose_catalogue_change(change.clone(), " ")),
            "reason : texte vide"
        );
        env.service
            .propose_catalogue_change(change.clone(), "Le maître ne fait plus de rapports.")
            .expect("proposed");
        let message = env.last_message();
        assert_eq!(
            message.text,
            "L'orchestrateur propose de désactiver la brique rapport. Le maître ne fait plus de rapports."
        );
        assert_eq!(
            message.actions,
            [MessageAction::ApplyCatalogueChange {
                change: change.clone(),
                decision: None
            }]
        );
        assert!(!env.service.catalogue()["rapport"].is_disabled);

        let decided = env
            .service
            .decide_catalogue_change(&message.id, CatalogueDecision::Applied)
            .expect("applied");
        assert!(env.service.catalogue()["rapport"].is_disabled);
        // The card shows the decision from then on, after a reload too.
        assert_eq!(
            decided.actions,
            [MessageAction::ApplyCatalogueChange {
                change,
                decision: Some(CatalogueDecision::Applied)
            }]
        );
        assert_eq!(env.last_message(), decided);
        assert_eq!(
            refusal(
                env.service
                    .decide_catalogue_change(&message.id, CatalogueDecision::Refused)
            ),
            "ce changement est déjà appliqué"
        );
    }

    #[tokio::test]
    async fn keeps_the_catalogue_as_it_is_when_the_master_refuses_a_change() {
        let env = Env::new();
        let change = CatalogueChange::Disable {
            id: "rapport".to_owned(),
        };
        env.service
            .propose_catalogue_change(change, "Plus de rapports.")
            .expect("proposed");
        let message = env.last_message();
        env.service
            .decide_catalogue_change(&message.id, CatalogueDecision::Refused)
            .expect("refused");
        assert!(!env.service.catalogue()["rapport"].is_disabled);
        assert_eq!(
            refusal(
                env.service
                    .decide_catalogue_change(&message.id, CatalogueDecision::Applied)
            ),
            "ce changement est déjà refusé"
        );
        assert_eq!(
            refusal(env.service.decide_catalogue_change("ghost", CatalogueDecision::Applied)),
            "message inconnu : ghost"
        );
    }
}

#[test]
fn shows_a_command_line_that_can_be_read_and_rerun() {
    assert_eq!(quote_argument("--model"), "--model");
    assert_eq!(quote_argument("/wt/proj-1"), "/wt/proj-1");
    assert_eq!(quote_argument("l'agent attend"), "'l'\\''agent attend'");
    assert_eq!(quote_argument(""), "''");
}

#[test]
fn reads_a_field_as_text_whatever_the_agent_handed() {
    let fields: HashMap<String, FieldValue> =
        serde_json::from_str(r#"{"a":" x ","b":2,"c":["u","v"],"d":[]}"#).expect("fields");
    assert_eq!(fields["a"], text(" x "));
    assert_eq!(field_text(fields.get("a")), "x");
    assert_eq!(field_text(fields.get("b")), "2");
    assert_eq!(field_text(fields.get("c")), "- u\n- v");
    assert_eq!(field_text(fields.get("d")), "none");
    assert_eq!(field_text(None), "");
}
