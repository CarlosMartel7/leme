//! The catalog of bundled skills and its validation.
//!
//! `agents/skills/catalog.toml` lists every bundled skill and every file in
//! it, with the file's size and SHA-256 hash. The build embeds the files
//! themselves (see `build.rs`). Before anything is installed, [`Catalog::new`]
//! checks that the embedded files and the catalog describe exactly the same
//! contents, so a damaged or hand-edited bundle is refused instead of copied.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The catalog format this Leme understands.
pub const CATALOG_SCHEMA_VERSION: u32 = 1;

/// One embedded file of a bundled skill.
#[derive(Debug, Clone, Copy)]
pub struct BundledFile {
    /// Directory name of the skill the file belongs to.
    pub skill: &'static str,
    /// Path inside the skill directory, with `/` separators.
    pub path: &'static str,
    /// The file's exact bytes.
    pub contents: &'static [u8],
}

mod bundled {
    use super::BundledFile;
    include!(concat!(env!("OUT_DIR"), "/bundled_skills.rs"));
}

/// Whether a skill was requested for installation or is needed by one that
/// was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillKind {
    /// One of the workflow skills Leme installs.
    Requested,
    /// A helper that another bundled skill invokes.
    Dependency,
}

/// A validated bundled skill.
#[derive(Debug, Clone)]
pub struct Skill {
    /// Installed directory name, equal to the skill's `name:`.
    pub name: String,
    pub kind: SkillKind,
    /// Bundled skills this one invokes.
    pub depends_on: Vec<String>,
    /// Tools, connectors or modes the skill expects at use time.
    pub requirements: Vec<String>,
    /// Every file of the skill, sorted by path.
    pub files: Vec<SkillFile>,
}

/// One file of a validated skill.
#[derive(Debug, Clone)]
pub struct SkillFile {
    /// Path inside the skill directory, with `/` separators.
    pub path: String,
    /// Lowercase hexadecimal SHA-256 of `contents`.
    pub sha256: String,
    pub contents: &'static [u8],
}

/// The validated set of bundled skills.
#[derive(Debug, Clone)]
pub struct Catalog {
    pub skills: Vec<Skill>,
}

impl Catalog {
    /// Validates the catalog and files embedded in this executable.
    pub fn bundled() -> Result<Self, CatalogError> {
        Self::new(bundled::CATALOG, bundled::FILES)
    }

    /// Validates `catalog` (the text of a `catalog.toml`) against `files`.
    ///
    /// Every catalog file must be present with the recorded size and hash,
    /// and every embedded file must be listed in the catalog.
    pub fn new(catalog: &str, files: &[BundledFile]) -> Result<Self, CatalogError> {
        let parsed: CatalogFile = toml::from_str(catalog).map_err(|error| {
            CatalogError::Invalid(format!("catalog.toml is not valid: {}", error.message()))
        })?;
        if parsed.schema_version != CATALOG_SCHEMA_VERSION {
            return Err(CatalogError::Invalid(format!(
                "catalog.toml has schema_version {}; this Leme reads version {CATALOG_SCHEMA_VERSION}",
                parsed.schema_version
            )));
        }

        let mut embedded: BTreeMap<(&str, &str), &'static [u8]> = BTreeMap::new();
        for file in files {
            if embedded
                .insert((file.skill, file.path), file.contents)
                .is_some()
            {
                return Err(CatalogError::Invalid(format!(
                    "{}/{} is embedded twice",
                    file.skill, file.path
                )));
            }
        }

        let names: BTreeSet<&str> = parsed.skills.iter().map(|s| s.name.as_str()).collect();
        if names.len() != parsed.skills.len() {
            return Err(CatalogError::Invalid(
                "catalog.toml lists a skill name twice".to_owned(),
            ));
        }

        let mut skills = Vec::with_capacity(parsed.skills.len());
        for entry in &parsed.skills {
            check_skill_name(&entry.name)?;
            for other in entry.depends_on.iter().chain(&entry.references) {
                if !names.contains(other.as_str()) {
                    return Err(CatalogError::Invalid(format!(
                        "skill {} refers to {other}, which is not bundled",
                        entry.name
                    )));
                }
            }
            let mut skill_files = Vec::with_capacity(entry.files.len());
            let mut seen = BTreeSet::new();
            for listed in &entry.files {
                check_file_path(&entry.name, &listed.path)?;
                if !seen.insert(listed.path.as_str()) {
                    return Err(CatalogError::Invalid(format!(
                        "skill {} lists {} twice",
                        entry.name, listed.path
                    )));
                }
                let contents = embedded
                    .remove(&(entry.name.as_str(), listed.path.as_str()))
                    .ok_or_else(|| {
                        CatalogError::Mismatch(format!(
                            "{}/{} is in the catalog but not in the bundle",
                            entry.name, listed.path
                        ))
                    })?;
                let sha256 = sha256_hex(contents);
                if contents.len() as u64 != listed.size || sha256 != listed.sha256 {
                    return Err(CatalogError::Mismatch(format!(
                        "{}/{} does not match its catalog size and SHA-256",
                        entry.name, listed.path
                    )));
                }
                skill_files.push(SkillFile {
                    path: listed.path.clone(),
                    sha256,
                    contents,
                });
            }
            if !seen.contains("SKILL.md") {
                return Err(CatalogError::Invalid(format!(
                    "skill {} has no SKILL.md",
                    entry.name
                )));
            }
            skill_files.sort_by(|a, b| a.path.cmp(&b.path));
            skills.push(Skill {
                name: entry.name.clone(),
                kind: entry.kind,
                depends_on: entry.depends_on.clone(),
                requirements: entry.requirements.clone(),
                files: skill_files,
            });
        }

        if let Some(((skill, path), _)) = embedded.into_iter().next() {
            return Err(CatalogError::Mismatch(format!(
                "{skill}/{path} is in the bundle but not in the catalog"
            )));
        }
        Ok(Self { skills })
    }
}

/// Lowercase hexadecimal SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A skill name is one plain directory name: lowercase letters, digits and
/// hyphens, not starting with a hyphen.
fn check_skill_name(name: &str) -> Result<(), CatalogError> {
    let valid = !name.is_empty()
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err(CatalogError::Invalid(format!(
            "skill name {name:?} must use only lowercase letters, digits and hyphens"
        )))
    }
}

/// A file path must stay inside its skill directory: relative, with only
/// ordinary components.
fn check_file_path(skill: &str, path: &str) -> Result<(), CatalogError> {
    let safe = !path.is_empty()
        && !path.contains('\\')
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && !path.split('/').any(str::is_empty);
    if safe {
        Ok(())
    } else {
        Err(CatalogError::Invalid(format!(
            "skill {skill} lists {path:?}, which is not a plain relative path inside the skill"
        )))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFile {
    schema_version: u32,
    #[allow(dead_code)] // Provenance, read by people.
    snapshot_date: String,
    skills: Vec<CatalogSkill>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)] // Provenance fields are read by people, not Leme.
struct CatalogSkill {
    name: String,
    kind: SkillKind,
    requested_as: Option<String>,
    source_path: String,
    source_name: String,
    depends_on: Vec<String>,
    references: Vec<String>,
    requirements: Vec<String>,
    files: Vec<CatalogFileEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFileEntry {
    path: String,
    size: u64,
    sha256: String,
}

/// The bundled skills cannot be trusted, so nothing is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    /// The catalog is malformed or breaks a rule.
    Invalid(String),
    /// The embedded files differ from the catalog.
    Mismatch(String),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) | Self::Mismatch(detail) => {
                write!(
                    f,
                    "the bundled skills are damaged, nothing was installed: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for CatalogError {}
