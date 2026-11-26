// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! How a local program finds and reaches the Atlas daemon of this machine: its folder, its port,
//! its shared secret, and plain HTTP requests on the loopback interface.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use atlas_i18n::tr;

/// Port the daemon listens on when `config.yml` sets none.
pub const DEFAULT_PORT: u16 = 4317;
/// Name the daemon gives on `/api/health`: tells it from another program on the same port.
pub const DAEMON_NAME: &str = "atlas";

const CONFIG_FILE: &str = "config.yml";
const TOKEN_FILE: &str = "token";

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LinkError {
    #[error("{}", tr!("daemon injoignable : {}", "daemon unreachable: {}", .0))]
    Unreachable(#[from] io::Error),
    #[error("{}", tr!("adresse du daemon invalide : {}", "invalid daemon address: {}", .0))]
    InvalidUrl(String),
    #[error("{}", tr!("réponse HTTP illisible", "unreadable HTTP response"))]
    InvalidResponse,
    #[error("{}", tr!("le daemon a répondu {}", "the daemon answered {}", .0))]
    Status(u16),
}

/// Folder of Atlas on this machine: `ATLAS_HOME`, else `~/.atlas`.
pub fn atlas_home() -> PathBuf {
    std::env::var_os("ATLAS_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".atlas"))
}

pub fn config_path(home: &Path) -> PathBuf {
    home.join(CONFIG_FILE)
}

pub fn token_path(home: &Path) -> PathBuf {
    home.join(TOKEN_FILE)
}

#[derive(serde::Deserialize)]
struct PortOnly {
    port: Option<u16>,
}

/// Port of `config.yml`, the default when the file is missing (first run) or does not set one.
pub fn configured_port(home: &Path) -> u16 {
    std::fs::read_to_string(config_path(home))
        .ok()
        .and_then(|source| serde_yaml_ng::from_str::<PortOnly>(&source).ok())
        .and_then(|config| config.port)
        .unwrap_or(DEFAULT_PORT)
}

pub fn daemon_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

/// Port of a daemon address such as `http://127.0.0.1:4317`.
pub fn port_of(url: &str) -> Result<u16, LinkError> {
    url.trim_end_matches('/')
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse().ok())
        .ok_or_else(|| LinkError::InvalidUrl(url.to_owned()))
}

#[derive(Debug, Clone, Default)]
pub struct Request<'a> {
    pub method: &'a str,
    pub path: &'a str,
    /// Shared secret, sent as a bearer token.
    pub token: Option<&'a str>,
    pub headers: &'a [(&'a str, &'a str)],
    /// JSON body, none for a request without one.
    pub body: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

/// Sends one HTTP request to the daemon on the loopback interface and reads its whole answer.
pub fn send(port: u16, request: &Request<'_>, timeout: Duration) -> Result<Response, LinkError> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let body = request.body.unwrap_or_default();
    let mut head = format!(
        "{} {} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n",
        request.method, request.path
    );
    if let Some(token) = request.token {
        head.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    for (name, value) in request.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if request.body.is_some() {
        head.push_str("Content-Type: application/json\r\n");
    }
    head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    parse_response(&String::from_utf8_lossy(&raw))
}

fn parse_response(raw: &str) -> Result<Response, LinkError> {
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((raw, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|status| status.parse().ok())
        .ok_or(LinkError::InvalidResponse)?;
    let is_chunked = head.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("transfer-encoding") && value.trim().eq_ignore_ascii_case("chunked")
        })
    });
    let body = if is_chunked {
        decode_chunked(body).ok_or(LinkError::InvalidResponse)?
    } else {
        body.to_owned()
    };
    Ok(Response { status, body })
}

/// A chunked body put back together; none when it is cut short or malformed.
fn decode_chunked(mut rest: &str) -> Option<String> {
    let mut decoded = String::new();
    loop {
        let (size_line, after) = rest.split_once("\r\n")?;
        let size = usize::from_str_radix(size_line.split(';').next()?.trim(), 16).ok()?;
        if size == 0 {
            return Some(decoded);
        }
        decoded.push_str(after.get(..size)?);
        rest = after.get(size..)?.strip_prefix("\r\n")?;
    }
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Version of the Atlas daemon that answers on `port`, none when nothing does or when it is another program.
pub fn running_version(port: u16) -> Option<String> {
    let request = Request {
        method: "GET",
        path: "/api/health",
        ..Request::default()
    };
    let Ok(Response { status: 200, body }) = send(port, &request, PROBE_TIMEOUT) else {
        return None;
    };
    let health: serde_json::Value = serde_json::from_str(&body).ok()?;
    (health["name"] == DAEMON_NAME).then(|| health["version"].as_str().unwrap_or_default().to_owned())
}

pub fn is_running(port: u16) -> bool {
    running_version(port).is_some()
}

/// The shared secret the daemon wrote when it first started, which every call to it must carry.
pub fn read_token(home: &Path) -> Result<String, LinkError> {
    Ok(std::fs::read_to_string(token_path(home))?.trim().to_owned())
}

/// Asks the daemon to stop: it closes the sessions of its agents and its database.
pub fn stop(port: u16, home: &Path) -> Result<(), LinkError> {
    let token = read_token(home)?;
    let request = Request {
        method: "POST",
        path: "/api/daemon/stop",
        token: Some(&token),
        ..Request::default()
    };
    match send(port, &request, PROBE_TIMEOUT)?.status {
        202 => Ok(()),
        status => Err(LinkError::Status(status)),
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    /// A server that answers one request with `response` and hands back what it received.
    fn serve_once(response: &'static str) -> (u16, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
        let port = listener.local_addr().expect("address").port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut received = Vec::new();
            let mut buffer = [0_u8; 1024];
            // Answering before the body is read would reset the connection under the client.
            while !is_whole_request(&String::from_utf8_lossy(&received)) {
                let count = stream.read(&mut buffer).expect("read");
                if count == 0 {
                    break;
                }
                received.extend_from_slice(&buffer[..count]);
            }
            stream.write_all(response.as_bytes()).expect("write");
            String::from_utf8_lossy(&received).into_owned()
        });
        (port, handle)
    }

    /// Whether `received` holds the headers of a request and the body they announce.
    fn is_whole_request(received: &str) -> bool {
        let Some((head, body)) = received.split_once("\r\n\r\n") else {
            return false;
        };
        let announced = head
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .and_then(|length| length.parse::<usize>().ok())
            .unwrap_or(0);
        body.len() >= announced
    }

    #[test]
    fn sends_the_token_headers_and_body_and_reads_the_answer() {
        let (port, server) = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\n{\"output\":{}}");
        let request = Request {
            method: "POST",
            path: "/hooks/stop",
            token: Some("secret"),
            headers: &[("X-Atlas-Agent", "agent-1")],
            body: Some("{}"),
        };
        let response = send(port, &request, PROBE_TIMEOUT).expect("response");
        assert_eq!(
            response,
            Response {
                status: 200,
                body: "{\"output\":{}}".into()
            }
        );
        let received = server.join().expect("server");
        assert!(received.starts_with("POST /hooks/stop HTTP/1.1\r\n"), "{received}");
        assert!(received.contains("Authorization: Bearer secret\r\n"));
        assert!(received.contains("X-Atlas-Agent: agent-1\r\n"));
        assert!(received.contains("Content-Length: 2\r\n"));
    }

    #[test]
    fn puts_a_chunked_answer_back_together() {
        let raw = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        assert_eq!(parse_response(raw).expect("response").body, "hello world");
    }

    #[test]
    fn tells_an_atlas_daemon_from_another_program() {
        let (atlas, _) =
            serve_once("HTTP/1.1 200 OK\r\nContent-Length: 34\r\n\r\n{\"name\":\"atlas\",\"version\":\"0.1.0\"}");
        assert!(is_running(atlas));
        let (versioned, _) =
            serve_once("HTTP/1.1 200 OK\r\nContent-Length: 34\r\n\r\n{\"name\":\"atlas\",\"version\":\"0.1.0\"}");
        assert_eq!(running_version(versioned).as_deref(), Some("0.1.0"));
        let (other, _) = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}");
        assert!(!is_running(other));
        let free = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("bind")
            .local_addr()
            .expect("address")
            .port();
        assert!(!is_running(free));
    }

    #[test]
    fn reads_the_port_of_the_configuration_and_of_an_address() {
        let home = std::env::temp_dir().join(format!("atlas-link-{}", std::process::id()));
        std::fs::create_dir_all(&home).expect("home");
        assert_eq!(configured_port(&home), DEFAULT_PORT);
        std::fs::write(config_path(&home), "port: 4400\nrepos: []\n").expect("config");
        assert_eq!(configured_port(&home), 4400);
        std::fs::remove_dir_all(&home).expect("cleanup");
        assert_eq!(port_of("http://127.0.0.1:4317").expect("port"), 4317);
        assert!(port_of("nonsense").is_err());
    }
}
