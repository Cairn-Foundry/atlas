// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! How to start a command without a shell, on every system.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCommand {
    pub file: String,
    /// Arguments to put before the caller's own arguments.
    pub prefix: Vec<String>,
}

impl ResolvedCommand {
    fn direct(file: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            prefix: Vec::new(),
        }
    }
}

const SHIM_EXTENSIONS: [&str; 2] = ["cmd", "bat"];
const SEARCHED_EXTENSIONS: [&str; 3] = ["exe", "cmd", "bat"];

/// Extension of a Windows path, lowercased; empty when it has none.
fn windows_extension(file: &str) -> String {
    let name = file.rsplit(['\\', '/']).next().unwrap_or(file);
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => extension.to_lowercase(),
        _ => String::new(),
    }
}

fn is_windows_absolute(file: &str) -> bool {
    let bytes = file.as_bytes();
    file.starts_with(['\\', '/']) || (bytes.len() > 2 && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/'))
}

/// How to start `command` on Windows. A command installed by npm is a `.cmd` shim that only
/// `cmd.exe` can run, while a native installer ships a `.exe`: going through `cmd.exe` only for
/// shims keeps free text away from its quoting rules. `path_variable` is the value of `PATH`.
pub fn resolve_windows_command(
    command: &str,
    shell: &str,
    path_variable: &str,
    exists: impl Fn(&str) -> bool,
) -> ResolvedCommand {
    let via_shell = |file: &str| ResolvedCommand {
        file: shell.to_owned(),
        prefix: vec!["/d".into(), "/s".into(), "/c".into(), file.to_owned()],
    };
    let resolved = |file: &str| {
        if SHIM_EXTENSIONS.contains(&windows_extension(file).as_str()) {
            via_shell(file)
        } else {
            ResolvedCommand::direct(file)
        }
    };
    if is_windows_absolute(command) || !windows_extension(command).is_empty() {
        return resolved(command);
    }
    for extension in SEARCHED_EXTENSIONS {
        for directory in path_variable.split(';').filter(|directory| !directory.is_empty()) {
            let candidate = format!("{}\\{command}.{extension}", directory.trim_end_matches('\\'));
            if exists(&candidate) {
                return resolved(&candidate);
            }
        }
    }
    via_shell(command)
}

/// How to start `command` without a shell on this system; `environment` is the one the command will run with.
pub fn resolve_command<'a>(
    command: &str,
    environment: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> ResolvedCommand {
    if !cfg!(windows) {
        return ResolvedCommand::direct(command);
    }
    let mut shell = "cmd.exe";
    let mut path_variable = "";
    for (key, value) in environment {
        if key == "ComSpec" {
            shell = value;
        } else if key.eq_ignore_ascii_case("PATH") {
            path_variable = value;
        }
    }
    resolve_windows_command(command, shell, path_variable, |candidate| Path::new(candidate).exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHELL: &str = "C:\\Windows\\cmd.exe";
    const PATH: &str = "C:\\tools;C:\\npm";

    fn via_shell(file: &str) -> ResolvedCommand {
        ResolvedCommand {
            file: SHELL.into(),
            prefix: vec!["/d".into(), "/s".into(), "/c".into(), file.into()],
        }
    }

    #[test]
    fn runs_a_native_executable_found_on_the_path_directly() {
        let resolved = resolve_windows_command("claude", SHELL, PATH, |path| path == "C:\\tools\\claude.exe");
        assert_eq!(resolved, ResolvedCommand::direct("C:\\tools\\claude.exe"));
    }

    #[test]
    fn prefers_an_executable_over_a_shim_whatever_the_path_order() {
        let resolved = resolve_windows_command("claude", SHELL, PATH, |path| {
            path == "C:\\tools\\claude.cmd" || path == "C:\\npm\\claude.exe"
        });
        assert_eq!(resolved, ResolvedCommand::direct("C:\\npm\\claude.exe"));
    }

    #[test]
    fn runs_an_npm_shim_through_the_shell() {
        let resolved = resolve_windows_command("claude", SHELL, PATH, |path| path == "C:\\npm\\claude.cmd");
        assert_eq!(resolved, via_shell("C:\\npm\\claude.cmd"));
    }

    #[test]
    fn takes_an_explicit_path_as_it_is() {
        assert_eq!(
            resolve_windows_command("C:\\bin\\claude.exe", SHELL, PATH, |_| false),
            ResolvedCommand::direct("C:\\bin\\claude.exe")
        );
        assert_eq!(
            resolve_windows_command("claude.CMD", SHELL, PATH, |_| false),
            via_shell("claude.CMD")
        );
    }

    #[test]
    fn falls_back_to_the_shell_for_a_command_found_nowhere() {
        assert_eq!(
            resolve_windows_command("claude", SHELL, PATH, |_| false),
            via_shell("claude")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn keeps_the_name_elsewhere() {
        assert_eq!(
            resolve_command("claude", [("PATH", "/usr/bin")]),
            ResolvedCommand::direct("claude")
        );
    }
}
