// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Running the daemon: loading what the master configured, serving the factory, stopping cleanly.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::catalogue::LiveCatalogue;
use crate::claude::{claude_dir, claude_state_path, process_env, write_secret};
use crate::clock::system_clock;
use crate::config::load_config;
use crate::language::{self, tr};
use crate::notifier::OsNotifier;
use crate::orchestrator::{Orchestrator, create_claude_runner};
use crate::server::{ServerDeps, build_router};
use crate::service::{AgentService, DEFAULT_CLOSE_DELAY, ServiceDeps};
use crate::store::Store;
use crate::terminal::PtyLauncher;
use crate::workflow::workflow_issues;

/// Longest the daemon waits for its clients to hang up once asked to stop.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// The secret shared between the daemon, the hooks and the MCP clients of the agents, created on first run.
pub fn load_token(home: &Path) -> anyhow::Result<String> {
    let path = atlas_link::token_path(home);
    if path.exists() {
        let token =
            std::fs::read_to_string(&path).with_context(|| tr!("lecture de {}", "reading {}", path.display()))?;
        // An empty file would make an empty bearer a valid secret: it gets a real one.
        if !token.trim().is_empty() {
            return Ok(token.trim().to_owned());
        }
    }
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    write_secret(&path, &token).with_context(|| tr!("écriture de {}", "writing {}", path.display()))?;
    Ok(token)
}

/// Resolves when the daemon is asked to stop: from a terminal, by the system, or by a client holding the token.
async fn stop_requested(stop: &Notify) {
    let interrupted = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminated = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminated = std::future::pending::<()>();
    tokio::select! {
        () = interrupted => {}
        () = terminated => {}
        () = stop.notified() => {}
    }
}

/// Keeps a second daemon off the registry of `home`: the lock lasts as long as the file stays open.
fn lock_home(home: &Path) -> anyhow::Result<std::fs::File> {
    let path = home.join("daemon.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .with_context(|| tr!("ouverture de {}", "opening {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => {
            anyhow::bail!(
                "{}",
                tr!(
                    "un autre daemon Atlas tient déjà {}",
                    "another Atlas daemon already holds {}",
                    home.display()
                )
            )
        }
        Err(std::fs::TryLockError::Error(error)) => {
            Err(error).with_context(|| tr!("verrouillage de {}", "locking {}", path.display()))
        }
    }
}

/// Runs the daemon of `home` until it is asked to stop.
pub async fn run(home: PathBuf) -> anyhow::Result<()> {
    let config = load_config(&home)?;
    // Both are taken before the registry is touched: recovering after a restart ends every session
    // on record, which a daemon still running on this home must never see happen under it.
    let _home_lock = lock_home(&home)?;
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, config.port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| tr!("écoute sur {address}", "listening on {address}"))?;
    let now = system_clock();
    let store = Store::open(&home.join("atlas.db"), Arc::clone(&now))
        .context(tr!("ouverture du registre", "opening the registry"))?;
    if let Some(picked) = language::picked(&store)? {
        language::speak(picked);
    }
    let catalogue = LiveCatalogue::load(&home)?;
    let issues = workflow_issues(&catalogue.current(), &config.default_steps, &[]);
    if !issues.is_empty() {
        anyhow::bail!(
            "{}",
            tr!(
                "defaultSteps de config.yml est invalide : {}",
                "defaultSteps of config.yml is invalid: {}",
                issues.join(" ; ")
            )
        );
    }
    let daemon_url = atlas_link::daemon_url(config.port);
    let token = load_token(&home)?;
    let hook_executable = std::env::current_exe()
        .context(tr!("chemin du daemon", "path of the daemon"))?
        .to_string_lossy()
        .into_owned();

    let service = AgentService::new(ServiceDeps {
        store: store.clone(),
        catalogue: catalogue.clone(),
        config: config.clone(),
        home: home.clone(),
        daemon_url: daemon_url.clone(),
        token: token.clone(),
        hook_executable,
        launcher: Box::new(PtyLauncher),
        notifier: Box::new(OsNotifier),
        claude_dir: claude_dir(),
        claude_state_path: claude_state_path(),
        environment: process_env(),
        now: Arc::clone(&now),
        close_delay: DEFAULT_CLOSE_DELAY,
    });
    service
        .recover_after_restart()
        .context(tr!("reprise après redémarrage", "recovering after a restart"))?;
    for repo in &config.repos {
        if let Err(error) = service.projects().add(repo.clone()).await {
            tracing::warn!(
                "{}",
                tr!(
                    "projet {} de config.yml ignoré : {error}",
                    "project {} of config.yml ignored: {error}",
                    repo.name
                )
            );
        }
    }
    let runner = create_claude_runner(&config, &home, &daemon_url, &token, &catalogue)
        .context(tr!("préparation de l'orchestrateur", "preparing the orchestrator"))?;
    let orchestrator = Orchestrator::new(store.clone(), service.clone(), runner, &config, now);
    orchestrator.rotate_chat_session().context(tr!(
        "rotation de la session de l'orchestrateur",
        "rotating the session of the orchestrator"
    ))?;
    orchestrator.listen();

    let stop = Arc::new(Notify::new());
    let router = build_router(ServerDeps {
        store,
        service: service.clone(),
        orchestrator,
        token,
        stop: Arc::clone(&stop),
    });

    let supervisor = service.clone();
    let interval = Duration::from_secs(config.supervisor_interval_seconds);
    tokio::spawn(async move {
        let mut ticks = tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
        loop {
            ticks.tick().await;
            if let Err(error) = supervisor.supervise() {
                tracing::error!("{}", tr!("supervision : {error}", "supervision: {error}"));
            }
        }
    });
    tracing::info!(
        "{}",
        tr!(
            "usine prête sur {daemon_url} (configuration : {})",
            "factory ready on {daemon_url} (configuration: {})",
            home.display()
        )
    );

    let hang_up = Arc::new(Notify::new());
    let server_hang_up = Arc::clone(&hang_up);
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move { server_hang_up.notified().await })
            .await
    });
    stop_requested(&stop).await;
    service.shutdown();
    hang_up.notify_one();
    // A client may keep its WebSockets open: it does not hold the daemon back.
    let _ = tokio::time::timeout(SHUTDOWN_GRACE, server).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_the_token_once_and_reads_it_back() {
        let home = tempfile::tempdir().expect("home");
        let token = load_token(home.path()).expect("token");
        assert_eq!(token.len(), 64);
        assert_eq!(load_token(home.path()).expect("again"), token);
        std::fs::write(home.path().join("token"), " \n").expect("emptied");
        assert_eq!(load_token(home.path()).expect("regenerated").len(), 64);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(home.path().join("token"))
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn refuses_a_home_another_daemon_holds() {
        let home = tempfile::tempdir().expect("home");
        let held = lock_home(home.path()).expect("first lock");
        let refused = lock_home(home.path()).expect_err("second lock").to_string();
        assert!(refused.contains("un autre daemon"), "{refused}");
        drop(held);
        lock_home(home.path()).expect("lock once released");
    }
}
