// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The catalogue of bricks in `~/.atlas/bricks`, seeded and kept up to date, with the skills and subagents the
//! bricks hand their sessions, from what Atlas ships.

use crate::language::tr;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use indexmap::IndexMap;
use sha2::{Digest, Sha256};

use crate::brick::{Brick, catalogue_issues};
use crate::config::{ConfigError, parse_yaml_file};
use crate::embedded::{DEFAULT_BRICKS, DEFAULT_SKILLS, DEFAULT_SUBAGENTS};
use crate::files::write_whole;

mod change;

pub use change::{apply_change, check_change, describe_change};

/// The bricks by id, in the order of their file names.
pub type Catalogue = IndexMap<String, Brick>;

const SCHEMA_FILE: &str = "brick.schema.json";
/// The JSON Schema of a brick file, derived from the type the daemon reads them into.
fn brick_schema() -> String {
    let schema = schemars::schema_for!(crate::brick::Brick);
    format!(
        "{}\n",
        serde_json::to_string_pretty(&schema).expect("a schema is plain JSON")
    )
}
/// Hash of each default brick as Atlas last wrote it: tells a brick the master edited from one he did not.
const DEFAULTS_FILE: &str = ".defaults.json";
/// Ids of the bricks taken out of service, in the bricks folder: a brick is disabled, never deleted,
/// so a shipped one does not come back with the next update.
const DISABLED_FILE: &str = ".disabled.json";
const BRICK_EXTENSION: &str = ".yml";
const BRICKS_DIR: &str = "bricks";
/// One folder per skill, holding its `SKILL.md`, as Claude Code lays them out.
pub const SKILLS_DIR: &str = "skills";
pub const SKILL_FILE: &str = "SKILL.md";
/// One `<name>.md` per subagent, as Claude Code lays them out.
pub const SUBAGENTS_DIR: &str = "subagents";
pub const SUBAGENT_EXTENSION: &str = ".md";

fn hash(content: &[u8]) -> String {
    Sha256::digest(content)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn io_error(path: &Path, error: std::io::Error) -> ConfigError {
    ConfigError(tr!("{} : {error}", "{}: {error}", path.display()))
}

fn parse_brick(path: &Path) -> Result<Brick, ConfigError> {
    let source = std::fs::read_to_string(path).map_err(|error| io_error(path, error))?;
    let file = path.to_string_lossy();
    let brick: Brick = parse_yaml_file(&source, &file, Brick::validated)?;
    if path.file_stem().and_then(|stem| stem.to_str()) != Some(brick.id.as_str()) {
        return Err(ConfigError(tr!(
            "{file} : le fichier doit porter le nom de son id ({}.yml)",
            "{file}: the file must carry the name of its id ({}.yml)",
            brick.id
        )));
    }
    Ok(brick)
}

/// A hidden file is never a brick: an editor's lock (`.#plan.yml`) or what macOS leaves on a
/// foreign volume (`._plan.yml`) would otherwise keep the whole catalogue from loading.
fn is_brick_file(name: &str) -> bool {
    name.ends_with(BRICK_EXTENSION) && !name.starts_with('.')
}

/// The skills of `home/skills` and the subagents of `home/subagents` a brick names but that are not there.
fn missing_resources(home: &Path, bricks: &[Brick]) -> Vec<String> {
    let mut issues = Vec::new();
    for brick in bricks {
        for skill in brick
            .skills
            .iter()
            .filter(|skill| !home.join(SKILLS_DIR).join(skill).join(SKILL_FILE).is_file())
        {
            issues.push(tr!(
                "{} : skills cite un skill absent de {SKILLS_DIR}/ ({skill})",
                "{}: skills names a skill missing from {SKILLS_DIR}/ ({skill})",
                brick.id
            ));
        }
        for subagent in brick.subagent_types.iter().filter(|subagent| {
            !home
                .join(SUBAGENTS_DIR)
                .join(format!("{subagent}{SUBAGENT_EXTENSION}"))
                .is_file()
        }) {
            issues.push(tr!(
                "{} : subagentTypes cite un sous-agent absent de {SUBAGENTS_DIR}/ ({subagent})",
                "{}: subagentTypes names a subagent missing from {SUBAGENTS_DIR}/ ({subagent})",
                brick.id
            ));
        }
    }
    issues
}

/// The ids listed in the disabled file of `dir`, none when it is absent.
fn read_disabled(dir: &Path) -> Result<Vec<String>, ConfigError> {
    let path = dir.join(DISABLED_FILE);
    match std::fs::read_to_string(&path) {
        Ok(source) => serde_json::from_str(&source).map_err(|error| {
            ConfigError(tr!(
                "{} : liste d'ids de briques attendue ({error})",
                "{}: list of brick ids expected ({error})",
                path.display()
            ))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(io_error(&path, error)),
    }
}

/// Reads every brick of `home/bricks` and checks the catalogue as a whole.
pub fn read_catalogue(home: &Path) -> Result<Catalogue, ConfigError> {
    let dir = &home.join(BRICKS_DIR);
    let disabled = read_disabled(dir)?;
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(|error| io_error(dir, error))?
        .flatten()
        .filter(|entry| is_brick_file(&entry.file_name().to_string_lossy()))
        .map(|entry| entry.path())
        .collect();
    files.sort();
    let mut bricks = files
        .iter()
        .map(|path| parse_brick(path))
        .collect::<Result<Vec<_>, _>>()?;
    for brick in &mut bricks {
        brick.is_disabled = disabled.contains(&brick.id);
    }
    if bricks.is_empty() {
        return Err(ConfigError(tr!(
            "{} ne contient aucune brique",
            "{} holds no brick",
            dir.display()
        )));
    }
    let mut issues = catalogue_issues(&bricks);
    issues.extend(missing_resources(home, &bricks));
    if !issues.is_empty() {
        let lines: Vec<String> = issues.iter().map(|issue| format!("  - {issue}")).collect();
        return Err(ConfigError(tr!(
            "catalogue de briques invalide ({}) :\n{}",
            "invalid catalogue of bricks ({}):\n{}",
            dir.display(),
            lines.join("\n")
        )));
    }
    Ok(bricks.into_iter().map(|brick| (brick.id.clone(), brick)).collect())
}

/// Brings the default files of `dir` up to date with `shipped`, by path relative to `dir`. A default
/// file arrives when it is missing, and follows the shipped one as long as the master has not edited
/// his copy; an edited file is never touched.
fn sync_defaults(dir: &Path, shipped: &[(&str, &[u8])]) -> Result<(), ConfigError> {
    let defaults_path = dir.join(DEFAULTS_FILE);
    let mut written: BTreeMap<String, String> = match std::fs::read_to_string(&defaults_path) {
        Ok(source) => serde_json::from_str(&source).unwrap_or_else(|error| {
            // Without it every default brick reads as edited by the master and stops following updates.
            tracing::warn!(
                "{}",
                tr!(
                    "{} illisible, briques par défaut figées : {error}",
                    "{} unreadable, default bricks frozen: {error}",
                    defaults_path.display()
                )
            );
            BTreeMap::new()
        }),
        Err(_) => BTreeMap::new(),
    };
    for (file, content) in shipped {
        let target = dir.join(file);
        let shipped_hash = hash(content);
        let current = std::fs::read(&target).ok().map(|bytes| hash(&bytes));
        let is_untouched = current.is_some() && current.as_ref() == written.get(*file);
        if current.is_none() || (is_untouched && current.as_ref() != Some(&shipped_hash)) {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
            }
            write_whole(&target, content).map_err(|error| io_error(&target, error))?;
        } else if current.as_ref() != Some(&shipped_hash) {
            continue;
        }
        written.insert((*file).to_owned(), shipped_hash);
    }
    // A default brick Atlas no longer ships: its file goes if the master never edited it, since it
    // may cite bricks that are gone too and would keep the whole catalogue from loading. An edited
    // one is his own brick from now on.
    let retired: Vec<String> = written
        .keys()
        .filter(|file| !shipped.iter().any(|(shipped_file, _)| shipped_file == file))
        .cloned()
        .collect();
    for file in retired {
        let target = dir.join(&file);
        let current = std::fs::read(&target).ok().map(|bytes| hash(&bytes));
        if current.is_some() && current.as_ref() == written.get(&file) {
            std::fs::remove_file(&target).map_err(|error| io_error(&target, error))?;
        }
        written.remove(&file);
    }
    let listing = serde_json::to_string_pretty(&written).map_err(|error| ConfigError(error.to_string()))?;
    write_whole(&defaults_path, format!("{listing}\n")).map_err(|error| io_error(&defaults_path, error))
}

/// Loads the catalogue of `home/bricks`, after bringing the default bricks, skills and subagents and
/// the JSON Schema of a brick up to date.
pub fn load_catalogue(home: &Path) -> Result<Catalogue, ConfigError> {
    for (folder, shipped) in [
        (BRICKS_DIR, DEFAULT_BRICKS),
        (SKILLS_DIR, DEFAULT_SKILLS),
        (SUBAGENTS_DIR, DEFAULT_SUBAGENTS),
    ] {
        let dir = home.join(folder);
        std::fs::create_dir_all(&dir).map_err(|error| io_error(&dir, error))?;
        sync_defaults(&dir, shipped)?;
    }
    let schema_path = home.join(BRICKS_DIR).join(SCHEMA_FILE);
    std::fs::write(&schema_path, brick_schema()).map_err(|error| io_error(&schema_path, error))?;
    read_catalogue(home)
}

/// Name, date and size of each brick file, and the skills and subagents present: tells whether what
/// the catalogue depends on changed since it was last read.
type Stamp = Vec<(String, Option<SystemTime>, u64)>;

fn stamp(home: &Path) -> Stamp {
    // A skill counts once its SKILL.md is there, which can come after its folder.
    let names = |folder: &str, inner: &str| -> Stamp {
        std::fs::read_dir(home.join(folder))
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| {
                let is_there = entry.path().join(inner).exists();
                (
                    format!("{folder}/{}", entry.file_name().to_string_lossy()),
                    None,
                    u64::from(is_there),
                )
            })
            .collect()
    };
    let mut files: Stamp = std::fs::read_dir(home.join(BRICKS_DIR))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            is_brick_file(&name.to_string_lossy()) || name == DISABLED_FILE
        })
        .map(|entry| {
            let metadata = entry.metadata().ok();
            (
                entry.file_name().to_string_lossy().into_owned(),
                metadata.as_ref().and_then(|metadata| metadata.modified().ok()),
                metadata.map_or(0, |metadata| metadata.len()),
            )
        })
        .collect();
    files.extend(names(SKILLS_DIR, SKILL_FILE));
    files.extend(names(SUBAGENTS_DIR, ""));
    files.sort();
    files
}

/// What a look at the bricks folder found.
#[derive(Debug, PartialEq, Eq)]
pub enum Refresh {
    Unchanged,
    Reloaded,
    /// The files changed into a catalogue that does not hold: the one in service stays, with why.
    Rejected(String),
}

struct LiveState {
    current: Arc<Catalogue>,
    /// The folder as it was last read, whether what it held was taken or refused.
    read: Stamp,
}

/// The catalogue in service, which follows the bricks folder: a brick the master adds or edits
/// counts from the next step that needs the catalogue, without restarting the daemon. Cheap to
/// clone: every clone follows the same folder.
#[derive(Clone)]
pub struct LiveCatalogue {
    home: PathBuf,
    state: Arc<Mutex<LiveState>>,
}

impl LiveCatalogue {
    /// Loads the catalogue of `home/bricks`; fails when it does not hold, since there is none to fall back on.
    pub fn load(home: &Path) -> Result<Self, ConfigError> {
        let current = Arc::new(load_catalogue(home)?);
        let read = stamp(home);
        Ok(Self {
            home: home.to_path_buf(),
            state: Arc::new(Mutex::new(LiveState { current, read })),
        })
    }

    pub fn current(&self) -> Arc<Catalogue> {
        Arc::clone(&self.state.lock().unwrap_or_else(PoisonError::into_inner).current)
    }

    /// Reads the folder again if its files changed. A change is reported once, taken or refused.
    pub fn refresh(&self) -> Refresh {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let seen = stamp(&self.home);
        if seen == state.read {
            return Refresh::Unchanged;
        }
        state.read = seen;
        match read_catalogue(&self.home) {
            Ok(catalogue) => {
                state.current = Arc::new(catalogue);
                Refresh::Reloaded
            }
            Err(error) => Refresh::Rejected(error.0),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn takes_no_hidden_file_for_a_brick() {
        assert!(is_brick_file("plan.yml"));
        assert!(!is_brick_file(".#plan.yml") && !is_brick_file("._plan.yml") && !is_brick_file("plan.yml.bak"));
    }

    /// The catalogue shipped with Atlas, as a fresh install loads it.
    pub(crate) fn shipped_catalogue() -> Catalogue {
        let home = tempfile::tempdir().expect("home");
        load_catalogue(home.path()).expect("catalogue")
    }

    #[test]
    fn loads_the_shipped_bricks_and_writes_the_schema() {
        let home = tempfile::tempdir().expect("home");
        let catalogue = load_catalogue(home.path()).expect("catalogue");
        let ids: Vec<&str> = catalogue.keys().map(String::as_str).collect();
        assert_eq!(
            ids,
            [
                "correction",
                "implementation",
                "plan",
                "qualification",
                "rapport",
                "review-mr",
                "review",
                "ship",
                "tache",
                "tests"
            ]
        );
        assert_eq!(catalogue["review"].fixed_by.as_deref(), Some("correction"));
        // What the review bricks hand their sessions arrives with them.
        assert_eq!(catalogue["review"].skills, ["deep-review"]);
        assert!(
            home.path()
                .join("skills/deep-review/references/calibration.md")
                .is_file()
        );
        let minos = std::fs::read_to_string(home.path().join("subagents/minos.md")).expect("minos");
        assert!(minos.contains("${CLAUDE_PLUGIN_ROOT}/skills/deep-review/references/") && !minos.contains("~/.claude"));
        let schema: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(home.path().join("bricks/brick.schema.json")).expect("schema"),
        )
        .expect("json");
        // What an editor needs to catch a typo while a brick is written.
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(
            schema["required"],
            serde_json::json!([
                "id",
                "name",
                "description",
                "model",
                "session",
                "thresholdMinutes",
                "writes",
                "instructions"
            ])
        );
        assert_eq!(schema["properties"]["id"]["pattern"], "^[a-z][a-z0-9-]*$");
        assert_eq!(
            schema["properties"]["writes"]["propertyNames"]["pattern"],
            "^[a-z][A-Za-z0-9]*$"
        );
        assert_eq!(schema["properties"]["thresholdMinutes"]["minimum"], 1);
    }

    const OLD: &[u8] = b"old default";
    const NEW: &[u8] = b"new default";

    #[test]
    fn follows_a_default_the_master_did_not_edit() {
        let dir = tempfile::tempdir().expect("dir");
        sync_defaults(dir.path(), &[("plan.yml", OLD)]).expect("first");
        sync_defaults(dir.path(), &[("plan.yml", NEW)]).expect("second");
        assert_eq!(std::fs::read(dir.path().join("plan.yml")).expect("file"), NEW);
    }

    #[test]
    fn never_touches_a_brick_the_master_edited() {
        let dir = tempfile::tempdir().expect("dir");
        sync_defaults(dir.path(), &[("plan.yml", OLD)]).expect("first");
        std::fs::write(dir.path().join("plan.yml"), "mine").expect("edit");
        sync_defaults(dir.path(), &[("plan.yml", NEW)]).expect("second");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("plan.yml")).expect("file"),
            "mine"
        );
    }

    #[test]
    fn refuses_a_brick_whose_file_does_not_carry_its_id() {
        let home = tempfile::tempdir().expect("home");
        load_catalogue(home.path()).expect("catalogue");
        let bricks = home.path().join("bricks");
        std::fs::copy(bricks.join("plan.yml"), bricks.join("other.yml")).expect("copy");
        let error = read_catalogue(home.path()).expect_err("error");
        assert!(
            error.0.contains("le fichier doit porter le nom de son id (plan.yml)"),
            "{}",
            error.0
        );
    }

    #[test]
    fn follows_a_default_in_a_subfolder() {
        let dir = tempfile::tempdir().expect("dir");
        sync_defaults(dir.path(), &[("deep-review/references/method.md", OLD)]).expect("first");
        sync_defaults(dir.path(), &[("deep-review/references/method.md", NEW)]).expect("second");
        assert_eq!(
            std::fs::read(dir.path().join("deep-review/references/method.md")).expect("file"),
            NEW
        );
    }

    #[test]
    fn refuses_a_brick_that_names_a_missing_skill_until_it_is_added() {
        let home = tempfile::tempdir().expect("home");
        let live = LiveCatalogue::load(home.path()).expect("catalogue");
        let plan = home.path().join("bricks/plan.yml");
        edit(&plan, "subagents: false", "subagents: true");
        edit(
            &plan,
            "requires: [qualification]",
            "requires: [qualification]\nskills: [house-style]\nsubagentTypes: [scribe]",
        );
        let Refresh::Rejected(reason) = live.refresh() else {
            panic!("a brick naming a missing skill was taken");
        };
        assert!(
            reason.contains("plan : skills cite un skill absent de skills/ (house-style)"),
            "{reason}"
        );
        assert!(
            reason.contains("plan : subagentTypes cite un sous-agent absent de subagents/ (scribe)"),
            "{reason}"
        );

        let skill = home.path().join("skills/house-style");
        std::fs::create_dir_all(&skill).expect("skill");
        std::fs::write(skill.join("SKILL.md"), "style").expect("SKILL.md");
        std::fs::write(home.path().join("subagents/scribe.md"), "scribe").expect("subagent");
        // Adding what was missing is enough: the brick file itself did not change.
        assert_eq!(live.refresh(), Refresh::Reloaded);
        assert_eq!(live.current()["plan"].skills, ["house-style"]);
    }

    #[test]
    fn removes_a_default_atlas_no_longer_ships_unless_the_master_edited_it() {
        let dir = tempfile::tempdir().expect("dir");
        let all = [("plan.yml", OLD), ("retired.yml", OLD), ("kept.yml", OLD)];
        sync_defaults(dir.path(), &all).expect("first");
        std::fs::write(dir.path().join("kept.yml"), "mine").expect("edit");
        sync_defaults(dir.path(), &[("plan.yml", OLD)]).expect("second");
        assert!(dir.path().join("plan.yml").exists());
        assert!(!dir.path().join("retired.yml").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("kept.yml")).expect("file"),
            "mine"
        );
        // The edited brick is the master's own from now on: a later sync leaves it alone.
        sync_defaults(dir.path(), &[("plan.yml", OLD)]).expect("third");
        assert!(dir.path().join("kept.yml").exists());
    }

    #[test]
    fn disables_a_brick_without_taking_it_from_the_catalogue() {
        let home = tempfile::tempdir().expect("home");
        let live = LiveCatalogue::load(home.path()).expect("catalogue");
        let disabled = home.path().join("bricks").join(DISABLED_FILE);
        std::fs::write(&disabled, r#"["rapport", "gone"]"#).expect("disable");
        assert_eq!(live.refresh(), Refresh::Reloaded);
        assert!(live.current()["rapport"].is_disabled && !live.current()["plan"].is_disabled);

        // The review loops on correction: correction cannot go while review is in service.
        std::fs::write(&disabled, r#"["correction"]"#).expect("disable");
        let Refresh::Rejected(reason) = live.refresh() else {
            panic!("correction was disabled under review");
        };
        assert!(
            reason.contains("review : fixedBy cite une brique désactivée (correction)"),
            "{reason}"
        );
        assert!(live.current()["rapport"].is_disabled);

        std::fs::write(&disabled, "rapport").expect("garbage");
        let Refresh::Rejected(reason) = live.refresh() else {
            panic!("an unreadable list was taken");
        };
        assert!(reason.contains("liste d'ids de briques attendue"), "{reason}");
    }

    fn edit(path: &Path, from: &str, to: &str) {
        let source = std::fs::read_to_string(path).expect("brick");
        assert!(source.contains(from), "{from}");
        std::fs::write(path, source.replace(from, to)).expect("edit");
    }

    #[test]
    fn follows_the_bricks_the_master_edits_and_keeps_the_catalogue_when_an_edit_does_not_hold() {
        let home = tempfile::tempdir().expect("home");
        let live = LiveCatalogue::load(home.path()).expect("catalogue");
        let plan = home.path().join("bricks/plan.yml");
        assert_eq!(live.refresh(), Refresh::Unchanged);

        edit(&plan, "name: Plan", "name: Plan détaillé");
        assert_eq!(live.refresh(), Refresh::Reloaded);
        assert_eq!(live.current()["plan"].name, "Plan détaillé");
        assert_eq!(live.refresh(), Refresh::Unchanged);

        edit(&plan, "name: Plan détaillé", "nom: Plan");
        let Refresh::Rejected(reason) = live.refresh() else {
            panic!("an invalid brick was taken");
        };
        assert!(reason.contains("plan.yml"), "{reason}");
        // Refused once: the catalogue the master had stays in service, and he is not told again.
        assert_eq!(live.current()["plan"].name, "Plan détaillé");
        assert_eq!(live.refresh(), Refresh::Unchanged);

        edit(&plan, "nom: Plan", "name: Plan");
        assert_eq!(live.refresh(), Refresh::Reloaded);
        assert_eq!(live.current()["plan"].name, "Plan");
    }
}
