// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The `hook` command: what Claude Code runs for the hooks of an agent session. It reads the hook
//! payload on stdin, asks the daemon what to do and prints its answer for Claude Code. It never
//! fails the agent: if the daemon is unreachable the hook stays silent.

use std::time::Duration;

use atlas_link::{Request, send};
use serde_json::Value;

const HOOK_TIMEOUT: Duration = Duration::from_secs(5);

/// What the daemon wants Claude Code to read for `event`; none when it has nothing to say or cannot be asked.
pub fn run_hook(event: &str, environment: impl Fn(&str) -> Option<String>, payload: &str) -> Option<String> {
    let url = environment("ATLAS_URL")?;
    let agent_id = environment("ATLAS_AGENT_ID")?;
    let token = environment("ATLAS_TOKEN")?;
    if event.is_empty() {
        return None;
    }
    let path = format!("/hooks/{event}");
    let request = Request {
        method: "POST",
        path: &path,
        token: Some(&token),
        headers: &[("X-Atlas-Agent", &agent_id)],
        body: Some(if payload.trim().is_empty() { "{}" } else { payload }),
    };
    let response = send(atlas_link::port_of(&url).ok()?, &request, HOOK_TIMEOUT).ok()?;
    if response.status != 200 {
        return None;
    }
    let body: Value = serde_json::from_str(&response.body).ok()?;
    let output = body.get("output").filter(|output| !output.is_null())?;
    serde_json::to_string(output).ok()
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpListener};

    use super::*;

    /// A daemon that answers one hook with `body` and hands back the request it received.
    fn daemon_answering(status_line: &'static str, body: &'static str) -> (u16, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
        let port = listener.local_addr().expect("address").port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut received = Vec::new();
            let mut buffer = [0_u8; 2048];
            while !String::from_utf8_lossy(&received).contains("\r\n\r\n{") {
                let count = stream.read(&mut buffer).expect("read");
                if count == 0 {
                    break;
                }
                received.extend_from_slice(&buffer[..count]);
            }
            let response = format!("{status_line}\r\nContent-Length: {}\r\n\r\n{body}", body.len());
            stream.write_all(response.as_bytes()).expect("write");
            String::from_utf8_lossy(&received).into_owned()
        });
        (port, handle)
    }

    fn environment(port: u16) -> impl Fn(&str) -> Option<String> {
        move |name| match name {
            "ATLAS_URL" => Some(format!("http://127.0.0.1:{port}")),
            "ATLAS_AGENT_ID" => Some("agent-1".to_owned()),
            "ATLAS_TOKEN" => Some("secret".to_owned()),
            _ => None,
        }
    }

    #[test]
    fn prints_what_the_daemon_answers_for_claude_code() {
        let (port, daemon) = daemon_answering(
            "HTTP/1.1 200 OK",
            r#"{"output":{"decision":"block","reason":"continue"}}"#,
        );
        let output = run_hook("stop", environment(port), "  ");
        assert_eq!(output.as_deref(), Some(r#"{"decision":"block","reason":"continue"}"#));
        let request = daemon.join().expect("daemon");
        assert!(request.starts_with("POST /hooks/stop HTTP/1.1\r\n"), "{request}");
        assert!(request.contains("Authorization: Bearer secret\r\n") && request.contains("X-Atlas-Agent: agent-1\r\n"));
        assert!(request.ends_with("\r\n\r\n{}"));
    }

    #[test]
    fn stays_silent_when_the_daemon_has_nothing_to_say_or_refuses() {
        let (quiet, _) = daemon_answering("HTTP/1.1 200 OK", r#"{"output":null}"#);
        assert_eq!(run_hook("notification", environment(quiet), "{}"), None);
        let (refusing, _) = daemon_answering("HTTP/1.1 401 Unauthorized", r#"{"error":"jeton manquant"}"#);
        assert_eq!(run_hook("stop", environment(refusing), "{}"), None);
    }

    #[test]
    fn stays_silent_outside_an_atlas_session_or_without_a_daemon() {
        assert_eq!(run_hook("stop", |_| None, "{}"), None);
        let free = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("bind")
            .local_addr()
            .expect("address")
            .port();
        assert_eq!(run_hook("stop", environment(free), "{}"), None);
        assert_eq!(run_hook("", environment(free), "{}"), None);
    }
}
