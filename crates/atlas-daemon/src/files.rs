// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Writing files other programs read while Atlas runs.

use std::path::Path;

/// Replaces `path` with `content` in one step: a reader, or a crash halfway, never meets a file
/// that is half written.
pub fn write_whole(path: &Path, content: impl AsRef<[u8]>) -> std::io::Result<()> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = path.with_file_name(format!(".{name}.tmp"));
    std::fs::write(&temporary, content)?;
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_a_file_and_leaves_nothing_beside_it() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("plan.yml");
        write_whole(&path, "first").expect("created");
        write_whole(&path, "second").expect("replaced");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "second");
        assert_eq!(std::fs::read_dir(dir.path()).expect("listing").count(), 1);
    }
}
