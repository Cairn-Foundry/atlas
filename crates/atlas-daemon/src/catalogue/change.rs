// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Changes to the catalogue the orchestrator proposes: checked before the master sees them, and again when he applies them.

use crate::language::tr;
use std::path::{Component, Path};

use crate::brick::{Brick, catalogue_issues};
use crate::config::parse_yaml_file;
use crate::contract::{CatalogueChange, is_brick_id};
use crate::error::{Error, Result, refuse};
use crate::files::write_whole;

use super::{
    BRICK_EXTENSION, BRICKS_DIR, Catalogue, DISABLED_FILE, SKILL_FILE, SKILLS_DIR, SUBAGENT_EXTENSION, SUBAGENTS_DIR,
    missing_resources, read_disabled,
};

/// Past this, a skill is not text a model wrote or copied: it is refused rather than shown.
const MAX_SKILL_BYTES: usize = 512 * 1024;

/// What the change does, as the master reads it in the chat: "ajouter la brique plan".
pub fn describe_change(home: &Path, catalogue: &Catalogue, change: &CatalogueChange) -> String {
    match change {
        CatalogueChange::Brick { id, .. } if catalogue.contains_key(id) => {
            tr!("remplacer la brique {id}", "replace the brick {id}")
        }
        CatalogueChange::Brick { id, .. } => tr!("ajouter la brique {id}", "add the brick {id}"),
        CatalogueChange::Skill { name, files } => {
            let verb = if home.join(SKILLS_DIR).join(name).exists() {
                tr!("remplacer", "replace")
            } else {
                tr!("installer", "install")
            };
            let count = files.len();
            let plural = if count > 1 { "s" } else { "" };
            tr!(
                "{verb} le skill {name} ({count} fichier{plural})",
                "{verb} the skill {name} ({count} file{plural})"
            )
        }
        CatalogueChange::Subagent { name, .. } => {
            let verb = if subagent_path(home, name).exists() {
                tr!("remplacer", "replace")
            } else {
                tr!("ajouter", "add")
            };
            tr!("{verb} le sous-agent {name}", "{verb} the subagent {name}")
        }
        CatalogueChange::Disable { id } => tr!("désactiver la brique {id}", "disable the brick {id}"),
        CatalogueChange::Enable { id } => tr!("réactiver la brique {id}", "enable the brick {id} again"),
    }
}

fn subagent_path(home: &Path, name: &str) -> std::path::PathBuf {
    home.join(SUBAGENTS_DIR).join(format!("{name}{SUBAGENT_EXTENSION}"))
}

fn require_name(field: &str, value: &str) -> Result<()> {
    if !is_brick_id(value) {
        refuse!(
            "{field} : lettres minuscules, chiffres et tirets ({value})",
            "{field}: lowercase letters, digits and dashes ({value})"
        );
    }
    Ok(())
}

/// A path inside the skill's folder: relative, `/`-separated, never climbing out of it.
fn is_inner_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && !path.contains(':')
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn require_known<'a>(catalogue: &'a Catalogue, id: &str) -> Result<&'a Brick> {
    catalogue
        .get(id)
        .ok_or_else(|| Error::refusal(tr!("brique inconnue : {id}", "unknown brick: {id}")))
}

/// Refuses the catalogue `bricks` would make, with every reason. The skills and subagents a brick
/// names are only required when it is applied: an import proposes them alongside, and the master
/// validates them first.
fn require_sound_catalogue(home: &Path, bricks: &[Brick], is_applying: bool) -> Result<()> {
    let mut issues = catalogue_issues(bricks);
    if is_applying {
        issues.extend(missing_resources(home, bricks));
    }
    if !issues.is_empty() {
        refuse!(
            "le catalogue ne tiendrait plus : {}",
            "the catalogue would no longer hold: {}",
            issues.join(" ; ")
        );
    }
    Ok(())
}

/// The brick a brick change writes, refused when its content does not hold.
fn proposed_brick(id: &str, content: &str) -> Result<Brick> {
    require_name("id", id)?;
    let brick: Brick = parse_yaml_file(content, &format!("{id}{BRICK_EXTENSION}"), Brick::validated)
        .map_err(|error| Error::refusal(error.0))?;
    if brick.id != id {
        refuse!(
            "le contenu porte l'id {}, pas {id}",
            "the content carries the id {}, not {id}",
            brick.id
        );
    }
    Ok(brick)
}

/// Refuses a change that would not hold once applied, in words the orchestrator can act on.
/// `default_steps` are the bricks of config.yml, which must stay in service.
pub fn check_change(
    home: &Path,
    catalogue: &Catalogue,
    default_steps: &[String],
    change: &CatalogueChange,
    is_applying: bool,
) -> Result<()> {
    match change {
        CatalogueChange::Brick { id, content } => {
            let mut brick = proposed_brick(id, content)?;
            brick.is_disabled = catalogue.get(id).is_some_and(|current| current.is_disabled);
            let mut bricks: Vec<Brick> = catalogue.values().filter(|known| &known.id != id).cloned().collect();
            bricks.push(brick);
            require_sound_catalogue(home, &bricks, is_applying)
        }
        CatalogueChange::Skill { name, files } => {
            require_name("name", name)?;
            if !files.contains_key(SKILL_FILE) {
                refuse!(
                    "un skill a besoin de son fichier {SKILL_FILE}",
                    "a skill needs its {SKILL_FILE} file"
                );
            }
            if let Some(path) = files.keys().find(|path| !is_inner_path(path)) {
                refuse!(
                    "chemin refusé : {path} (relatif au dossier du skill, sans ..)",
                    "path refused: {path} (relative to the folder of the skill, without ..)"
                );
            }
            let size: usize = files.values().map(String::len).sum();
            if size > MAX_SKILL_BYTES {
                refuse!(
                    "skill trop lourd : {size} octets, {MAX_SKILL_BYTES} au plus",
                    "skill too large: {size} bytes, {MAX_SKILL_BYTES} at most"
                );
            }
            Ok(())
        }
        CatalogueChange::Subagent { name, content } => {
            require_name("name", name)?;
            if content.trim().is_empty() {
                refuse!("content : texte vide", "content: empty text");
            }
            Ok(())
        }
        CatalogueChange::Disable { id } => {
            if require_known(catalogue, id)?.is_disabled {
                refuse!(
                    "la brique {id} est déjà désactivée",
                    "the brick {id} is already disabled"
                );
            }
            if default_steps.contains(id) {
                refuse!(
                    "la brique {id} fait partie de defaultSteps dans config.yml : retirez-la d'abord",
                    "the brick {id} is part of defaultSteps in config.yml: remove it first"
                );
            }
            let bricks: Vec<Brick> = catalogue
                .values()
                .cloned()
                .map(|mut brick| {
                    brick.is_disabled |= &brick.id == id;
                    brick
                })
                .collect();
            require_sound_catalogue(home, &bricks, is_applying)
        }
        CatalogueChange::Enable { id } => {
            if !require_known(catalogue, id)?.is_disabled {
                refuse!(
                    "la brique {id} est déjà en service",
                    "the brick {id} is already in service"
                );
            }
            Ok(())
        }
    }
}

fn write_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(write_whole(path, content)?)
}

fn write_disabled(home: &Path, change: impl FnOnce(&mut Vec<String>)) -> Result<()> {
    let dir = home.join(BRICKS_DIR);
    let mut disabled = read_disabled(&dir).map_err(|error| Error::refusal(error.0))?;
    change(&mut disabled);
    disabled.sort();
    disabled.dedup();
    write_file(
        &dir.join(DISABLED_FILE),
        &format!("{}\n", serde_json::to_string_pretty(&disabled)?),
    )
}

/// Checks the change against the catalogue as it is now, then writes it in `home`.
pub fn apply_change(
    home: &Path,
    catalogue: &Catalogue,
    default_steps: &[String],
    change: &CatalogueChange,
) -> Result<()> {
    check_change(home, catalogue, default_steps, change, true)?;
    match change {
        CatalogueChange::Brick { id, content } => {
            write_file(&home.join(BRICKS_DIR).join(format!("{id}{BRICK_EXTENSION}")), content)
        }
        CatalogueChange::Skill { name, files } => {
            let dir = home.join(SKILLS_DIR).join(name);
            // The skill is replaced whole: a file the new version dropped must not linger.
            if dir.exists() {
                std::fs::remove_dir_all(&dir)?;
            }
            files
                .iter()
                .try_for_each(|(path, content)| write_file(&dir.join(path), content))
        }
        CatalogueChange::Subagent { name, content } => write_file(&subagent_path(home, name), content),
        CatalogueChange::Disable { id } => write_disabled(home, |disabled| disabled.push(id.clone())),
        CatalogueChange::Enable { id } => write_disabled(home, |disabled| disabled.retain(|known| known != id)),
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::*;
    use crate::catalogue::LiveCatalogue;

    struct Home {
        dir: tempfile::TempDir,
        live: LiveCatalogue,
    }

    impl Home {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("home");
            let live = LiveCatalogue::load(dir.path()).expect("catalogue");
            Self { dir, live }
        }

        fn check(&self, change: &CatalogueChange) -> Result<()> {
            check_change(
                self.dir.path(),
                &self.live.current(),
                &["plan".to_owned()],
                change,
                false,
            )
        }

        fn apply(&self, change: &CatalogueChange) -> Result<()> {
            apply_change(self.dir.path(), &self.live.current(), &["plan".to_owned()], change)?;
            self.live.refresh();
            Ok(())
        }

        fn describe(&self, change: &CatalogueChange) -> String {
            describe_change(self.dir.path(), &self.live.current(), change)
        }
    }

    fn refusal(result: Result<()>) -> String {
        result.expect_err("refused").to_string()
    }

    fn brick_yaml(id: &str, extra: &str) -> String {
        format!(
            "id: {id}\nname: Audit\ndescription: Un audit\nmodel: opus\nsession: audit\nthresholdMinutes: 10\nwrites: {{ audit: L'audit }}\ninstructions: Audite\n{extra}"
        )
    }

    #[test]
    fn adds_a_brick_once_its_skill_is_there() {
        let home = Home::new();
        let brick = CatalogueChange::Brick {
            id: "audit".to_owned(),
            content: brick_yaml("audit", "skills: [house-style]\n"),
        };
        assert_eq!(home.describe(&brick), "ajouter la brique audit");
        // Proposed with its skill, it is shown; applied before the skill, it is refused.
        home.check(&brick).expect("proposed");
        assert!(refusal(home.apply(&brick)).contains("skills cite un skill absent de skills/ (house-style)"));

        let skill = CatalogueChange::Skill {
            name: "house-style".to_owned(),
            files: IndexMap::from([
                ("SKILL.md".to_owned(), "---\nname: house-style\n---\nStyle".to_owned()),
                ("references/rules.md".to_owned(), "Règles".to_owned()),
            ]),
        };
        assert_eq!(home.describe(&skill), "installer le skill house-style (2 fichiers)");
        home.apply(&skill).expect("skill");
        home.apply(&brick).expect("brick");
        assert_eq!(home.live.current()["audit"].skills, ["house-style"]);
        assert!(home.dir.path().join("skills/house-style/references/rules.md").is_file());
        assert_eq!(home.describe(&brick), "remplacer la brique audit");
    }

    #[test]
    fn refuses_a_brick_that_does_not_hold() {
        let home = Home::new();
        let mismatch = CatalogueChange::Brick {
            id: "audit".to_owned(),
            content: brick_yaml("other", ""),
        };
        assert_eq!(refusal(home.check(&mismatch)), "le contenu porte l'id other, pas audit");
        let broken = CatalogueChange::Brick {
            id: "audit".to_owned(),
            content: brick_yaml("audit", "requires: [ghost]\n"),
        };
        assert!(refusal(home.check(&broken)).contains("audit : requires cite une brique inconnue (ghost)"));
        let typo = CatalogueChange::Brick {
            id: "audit".to_owned(),
            content: brick_yaml("audit", "treshold: 3\n"),
        };
        assert!(refusal(home.check(&typo)).contains("treshold"));
    }

    #[test]
    fn keeps_a_skill_inside_its_folder() {
        let home = Home::new();
        let skill = |path: &str| CatalogueChange::Skill {
            name: "house-style".to_owned(),
            files: IndexMap::from([
                ("SKILL.md".to_owned(), "Style".to_owned()),
                (path.to_owned(), "x".to_owned()),
            ]),
        };
        for path in ["../escape.md", "/etc/passwd", "a\\b.md", "C:/x.md", "a/../../b.md", ""] {
            assert!(refusal(home.check(&skill(path))).starts_with("chemin refusé"), "{path}");
        }
        let without_entry = CatalogueChange::Skill {
            name: "house-style".to_owned(),
            files: IndexMap::from([("README.md".to_owned(), "x".to_owned())]),
        };
        assert_eq!(
            refusal(home.check(&without_entry)),
            "un skill a besoin de son fichier SKILL.md"
        );
    }

    #[test]
    fn disables_and_enables_a_brick_without_deleting_it() {
        let home = Home::new();
        let disable = |id: &str| CatalogueChange::Disable { id: id.to_owned() };
        assert!(refusal(home.check(&disable("plan"))).contains("defaultSteps"));
        assert!(
            refusal(home.check(&disable("correction"))).contains("fixedBy cite une brique désactivée (correction)")
        );
        assert_eq!(refusal(home.check(&disable("ghost"))), "brique inconnue : ghost");

        home.apply(&disable("rapport")).expect("disable");
        assert!(home.live.current()["rapport"].is_disabled);
        assert!(home.dir.path().join("bricks/rapport.yml").is_file());
        assert_eq!(
            refusal(home.check(&disable("rapport"))),
            "la brique rapport est déjà désactivée"
        );

        let enable = CatalogueChange::Enable {
            id: "rapport".to_owned(),
        };
        assert_eq!(home.describe(&enable), "réactiver la brique rapport");
        home.apply(&enable).expect("enable");
        assert!(!home.live.current()["rapport"].is_disabled);
        assert_eq!(refusal(home.check(&enable)), "la brique rapport est déjà en service");
    }

    #[test]
    fn writes_a_subagent() {
        let home = Home::new();
        let subagent = CatalogueChange::Subagent {
            name: "scribe".to_owned(),
            content: "---\nname: scribe\n---\nÉcris.".to_owned(),
        };
        assert_eq!(home.describe(&subagent), "ajouter le sous-agent scribe");
        home.apply(&subagent).expect("subagent");
        assert!(home.dir.path().join("subagents/scribe.md").is_file());
        assert_eq!(home.describe(&subagent), "remplacer le sous-agent scribe");
    }
}
