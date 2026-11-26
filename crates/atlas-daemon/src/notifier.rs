// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Native notifications: how the master learns that an agent needs him while Atlas is out of sight.

use crate::language::tr;

pub trait Notifier: Send + Sync {
    fn notify(&self, title: &str, body: &str);
}

pub struct OsNotifier;

#[cfg(target_os = "linux")]
impl Notifier for OsNotifier {
    fn notify(&self, title: &str, body: &str) {
        use std::process::{Command, Stdio};

        let spawned = Command::new("notify-send")
            // `--` ends the options: a body opening on a dash, as a list item does, is still a text.
            .args(["--app-name=Atlas", "--", title, body])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match spawned {
            // Reaped on a thread of its own, so a slow notification server never holds the factory back.
            Ok(mut child) => {
                std::thread::spawn(move || match child.wait() {
                    Ok(status) if !status.success() => tracing::warn!(
                        "{}",
                        tr!("notification refusée : {status}", "notification refused: {status}")
                    ),
                    Ok(_) => {}
                    Err(error) => tracing::warn!(
                        "{}",
                        tr!("notification impossible : {error}", "notification failed: {error}")
                    ),
                });
            }
            Err(error) => tracing::warn!(
                "{}",
                tr!("notification impossible : {error}", "notification failed: {error}")
            ),
        }
    }
}

#[cfg(not(target_os = "linux"))]
impl Notifier for OsNotifier {
    fn notify(&self, title: &str, body: &str) {
        let (title, body) = (title.to_owned(), body.to_owned());
        // On a thread of its own: macOS waits up to two seconds for the delivery, which would hold
        // the factory back at every warning.
        std::thread::spawn(move || {
            if let Err(error) = notify_rust::Notification::new()
                .appname("Atlas")
                .summary(&title)
                .body(&body)
                .show()
            {
                tracing::warn!(
                    "{}",
                    tr!("notification impossible : {error}", "notification failed: {error}")
                );
            }
        });
    }
}
