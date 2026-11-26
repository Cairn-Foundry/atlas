// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Embeds the default bricks, skills and subagents into the daemon binary, so the daemon ships as one file.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Every file under `dir`, with its path relative to it, in a stable order.
fn files_under(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if let Ok(relative) = path.strip_prefix(dir) {
                found.push((relative.to_string_lossy().replace('\\', "/"), path));
            }
        }
    }
    found.sort();
    found
}

fn embed(name: &str, files: &[(String, PathBuf)], generated: &mut String) {
    writeln!(generated, "pub static {name}: &[(&str, &[u8])] = &[").expect("write");
    for (relative, path) in files {
        writeln!(
            generated,
            "    ({relative:?}, include_bytes!({:?})),",
            path.display().to_string()
        )
        .expect("write");
    }
    writeln!(generated, "];").expect("write");
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let mut generated = String::new();
    for (name, folder, extension) in [
        ("DEFAULT_BRICKS", "bricks", ".yml"),
        ("DEFAULT_SKILLS", "skills", ""),
        ("DEFAULT_SUBAGENTS", "subagents", ".md"),
    ] {
        let dir = manifest_dir.join(folder);
        println!("cargo:rerun-if-changed={}", dir.display());
        let files: Vec<_> = files_under(&dir)
            .into_iter()
            .filter(|(relative, _)| relative.ends_with(extension))
            .collect();
        embed(name, &files, &mut generated);
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out_dir.join("embedded.rs"), generated).expect("write embedded.rs");
}
