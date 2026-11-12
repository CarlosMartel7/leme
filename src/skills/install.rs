//! Installing the bundled skills into an agent's user skills directory.
//!
//! Installation runs in two phases. The preflight inspects every selected
//! destination without writing and decides, for each skill, whether to
//! install it, update it, leave it unchanged, or refuse because the existing
//! directory is not Leme's to replace. Any refusal stops the whole run before
//! a single file is written. Only then are the targets written, one after the
//! other, each under a lock and after checking its destination again.
//!
//! A skill directory is replaced as a whole: the new copy is written beside
//! it and renamed into place. `.leme-skills.toml` in each skills directory
//! records what Leme installed there, which is how an older Leme copy (safe
//! to update) is told apart from local edits (refused). A directory already
//! identical to the bundled skill is adopted as installed.
//!
//! Nothing here runs skill content, or touches agent settings, permissions,
//! authentication or connectors.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::catalog::{Catalog, Skill, SkillKind, sha256_hex};

/// Name of the file recording Leme-installed skills in a skills directory.
pub const MANIFEST_FILE: &str = ".leme-skills.toml";
/// Lock held while one Leme process writes a skills directory.
const LOCK_FILE: &str = ".leme-skills.lock";
/// The manifest format this Leme reads and writes.
const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// An agent whose user skills directory can receive the bundled skills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Codex,
    Claude,
}

impl Target {
    /// The agent's product name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
        }
    }
}

/// The environment values that decide where skills go.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    /// `HOME`.
    pub home: Option<OsString>,
    /// `CLAUDE_CONFIG_DIR`, which relocates Claude Code's `~/.claude`.
    pub claude_config_dir: Option<OsString>,
}

impl Environment {
    /// Reads the values from this process's environment.
    pub fn from_process() -> Self {
        Self {
            home: std::env::var_os("HOME"),
            claude_config_dir: std::env::var_os("CLAUDE_CONFIG_DIR"),
        }
    }
}

/// The user-scope skills directory `target` reads.
///
/// Codex reads `$HOME/.agents/skills`. Claude Code reads
/// `~/.claude/skills`, where `~/.claude` moves to `CLAUDE_CONFIG_DIR` when
/// that is set. Empty variables count as unset; relative paths are refused,
/// because the agent would resolve them against its own working directory.
pub fn skills_root(target: Target, environment: &Environment) -> Result<PathBuf, InstallError> {
    let set =
        |value: &Option<OsString>| value.as_ref().filter(|v| !v.is_empty()).map(PathBuf::from);
    if target == Target::Claude
        && let Some(config) = set(&environment.claude_config_dir)
    {
        return if config.is_absolute() {
            Ok(config.join("skills"))
        } else {
            Err(InstallError::RelativeVariable {
                variable: "CLAUDE_CONFIG_DIR",
                value: config,
            })
        };
    }
    let home = set(&environment.home).ok_or(InstallError::NoHome)?;
    if !home.is_absolute() {
        return Err(InstallError::RelativeVariable {
            variable: "HOME",
            value: home,
        });
    }
    Ok(match target {
        Target::Codex => home.join(".agents").join("skills"),
        Target::Claude => home.join(".claude").join("skills"),
    })
}

/// What the preflight decided for one skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// The destination does not exist yet.
    Install,
    /// The destination holds an unmodified older Leme copy.
    Update,
    /// The destination already matches the bundled skill.
    Unchanged,
    /// The destination exists and is not Leme's to replace.
    Conflict,
}

/// Why an existing destination is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Conflict {
    /// A different directory that Leme did not install.
    Unmanaged,
    /// A Leme copy whose files were changed afterwards.
    LocallyModified,
    /// A file or symbolic link where the skill directory belongs.
    NotADirectory,
    /// A directory containing a symbolic link or special file.
    UnsupportedEntry,
}

impl Conflict {
    /// One sentence for the user.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Unmanaged => "a different skill directory that Leme did not install",
            Self::LocallyModified => "installed by Leme, then changed locally",
            Self::NotADirectory => "not a plain directory (a file or symbolic link)",
            Self::UnsupportedEntry => "contains a symbolic link or special file",
        }
    }
}

/// What happened to one skill when its target was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillResult {
    Installed,
    Updated,
    Unchanged,
    Failed,
    /// An earlier failure stopped this target first.
    NotAttempted,
}

/// The overall outcome of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Dry run without conflicts: nothing was written.
    Planned,
    /// Some destinations conflict: nothing was written.
    Conflict,
    /// Every selected target was written.
    Installed,
    /// At least one target was written and at least one failed.
    Partial,
    /// No target could be written.
    Failed,
}

/// The whole result, shared by the text and JSON output.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub dry_run: bool,
    pub outcome: Outcome,
    pub targets: Vec<TargetReport>,
}

/// The result for one target.
#[derive(Debug, Clone, Serialize)]
pub struct TargetReport {
    pub target: Target,
    /// The skills directory written.
    pub root: PathBuf,
    /// Why the target could not be completed, if it was not.
    pub error: Option<String>,
    pub skills: Vec<SkillReport>,
}

/// The result for one skill in one target.
#[derive(Debug, Clone, Serialize)]
pub struct SkillReport {
    pub name: String,
    pub kind: SkillKind,
    pub destination: PathBuf,
    pub action: Action,
    pub conflict: Option<Conflict>,
    /// Absent until the target is written; dry runs and refused runs have none.
    pub result: Option<SkillResult>,
    pub requirements: Vec<String>,
}

/// Installs every catalog skill for each target, or with `dry_run` reports
/// what would happen.
///
/// Errors that prevent the preflight return `Err` and nothing is written.
/// Conflicts and write failures are part of the returned report.
pub fn install(
    catalog: &Catalog,
    targets: &[Target],
    environment: &Environment,
    dry_run: bool,
) -> Result<Report, InstallError> {
    let mut plans = Vec::with_capacity(targets.len());
    for &target in targets {
        let root = skills_root(target, environment)?;
        plans.push(TargetPlan::inspect(catalog, target, root)?);
    }

    let conflicted = plans.iter().any(TargetPlan::has_conflict);
    if conflicted || dry_run {
        return Ok(Report {
            dry_run,
            outcome: if conflicted {
                Outcome::Conflict
            } else {
                Outcome::Planned
            },
            targets: plans.iter().map(|plan| plan.report(catalog)).collect(),
        });
    }

    let reports: Vec<TargetReport> = plans
        .iter()
        .map(|plan| write_target(catalog, plan.target, &plan.root))
        .collect();
    let failed = reports.iter().filter(|r| r.error.is_some()).count();
    let outcome = match failed {
        0 => Outcome::Installed,
        n if n == reports.len() => Outcome::Failed,
        _ => Outcome::Partial,
    };
    Ok(Report {
        dry_run,
        outcome,
        targets: reports,
    })
}

/// The preflight decisions for one target.
#[derive(Debug)]
struct TargetPlan {
    target: Target,
    root: PathBuf,
    manifest: Manifest,
    /// One decision per catalog skill, in catalog order.
    decisions: Vec<(Action, Option<Conflict>)>,
}

impl TargetPlan {
    /// Inspects `root` without writing anything.
    fn inspect(catalog: &Catalog, target: Target, root: PathBuf) -> Result<Self, InstallError> {
        match fs::metadata(&root) {
            Ok(metadata) if !metadata.is_dir() => {
                return Err(InstallError::RootNotADirectory(root));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(InstallError::Read { path: root, source }),
        }
        let manifest = Manifest::read(&root)?;
        let mut decisions = Vec::with_capacity(catalog.skills.len());
        for skill in &catalog.skills {
            decisions.push(decide(skill, &root.join(&skill.name), &manifest)?);
        }
        Ok(Self {
            target,
            root,
            manifest,
            decisions,
        })
    }

    fn has_conflict(&self) -> bool {
        self.decisions
            .iter()
            .any(|(action, _)| *action == Action::Conflict)
    }

    fn report(&self, catalog: &Catalog) -> TargetReport {
        TargetReport {
            target: self.target,
            root: self.root.clone(),
            error: None,
            skills: catalog
                .skills
                .iter()
                .zip(&self.decisions)
                .map(|(skill, &(action, conflict))| SkillReport {
                    name: skill.name.clone(),
                    kind: skill.kind,
                    destination: self.root.join(&skill.name),
                    action,
                    conflict,
                    result: None,
                    requirements: skill.requirements.clone(),
                })
                .collect(),
        }
    }
}

/// Decides what to do with one skill's destination.
fn decide(
    skill: &Skill,
    destination: &Path,
    manifest: &Manifest,
) -> Result<(Action, Option<Conflict>), InstallError> {
    let metadata = match fs::symlink_metadata(destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((Action::Install, None));
        }
        Err(source) => {
            return Err(InstallError::Read {
                path: destination.to_path_buf(),
                source,
            });
        }
    };
    if !metadata.is_dir() {
        return Ok((Action::Conflict, Some(Conflict::NotADirectory)));
    }
    let Some(found) = read_tree(destination)? else {
        return Ok((Action::Conflict, Some(Conflict::UnsupportedEntry)));
    };
    if found == bundled_tree(skill) {
        return Ok((Action::Unchanged, None));
    }
    Ok(match manifest.record(&skill.name) {
        Some(record) if record == found => (Action::Update, None),
        Some(_) => (Action::Conflict, Some(Conflict::LocallyModified)),
        None => (Action::Conflict, Some(Conflict::Unmanaged)),
    })
}

/// A skill directory's contents: relative path to SHA-256.
type Tree = BTreeMap<String, String>;

fn bundled_tree(skill: &Skill) -> Tree {
    skill
        .files
        .iter()
        .map(|file| (file.path.clone(), file.sha256.clone()))
        .collect()
}

/// Reads every regular file below `directory` without following links.
/// Returns `None` when it holds a symbolic link, special file or a name that
/// is not UTF-8, none of which Leme ever installs.
fn read_tree(directory: &Path) -> Result<Option<Tree>, InstallError> {
    let mut tree = Tree::new();
    let mut pending = vec![(directory.to_path_buf(), String::new())];
    while let Some((path, prefix)) = pending.pop() {
        let entries = fs::read_dir(&path).map_err(|source| InstallError::Read {
            path: path.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| InstallError::Read {
                path: path.clone(),
                source,
            })?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                return Ok(None);
            };
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let inner = entry.path();
            let kind = entry.file_type().map_err(|source| InstallError::Read {
                path: inner.clone(),
                source,
            })?;
            if kind.is_dir() {
                pending.push((inner, relative));
            } else if kind.is_file() {
                let bytes = fs::read(&inner).map_err(|source| InstallError::Read {
                    path: inner.clone(),
                    source,
                })?;
                tree.insert(relative, sha256_hex(&bytes));
            } else {
                return Ok(None);
            }
        }
    }
    Ok(Some(tree))
}

/// Writes one target. The destination is inspected again under the lock, so
/// a change since the preflight is refused rather than overwritten.
fn write_target(catalog: &Catalog, target: Target, root: &Path) -> TargetReport {
    let mut report = TargetReport {
        target,
        root: root.to_path_buf(),
        error: None,
        skills: Vec::new(),
    };
    let plan = fs::create_dir_all(root)
        .map_err(|source| InstallError::Write {
            path: root.to_path_buf(),
            source,
        })
        .and_then(|()| lock(root))
        .and_then(|lock| {
            Ok((
                lock,
                TargetPlan::inspect(catalog, target, root.to_path_buf())?,
            ))
        });
    let (_lock, mut plan) = match plan {
        Ok(found) => found,
        Err(error) => {
            report.error = Some(error.to_string());
            report.skills = TargetPlan::unattempted(catalog, root);
            return report;
        }
    };
    report.skills = plan.report(catalog).skills;
    if plan.has_conflict() {
        report.error = Some(
            "the skills directory changed after the preflight; nothing was written here".to_owned(),
        );
        return report;
    }

    let mut failure = None;
    for (index, skill) in catalog.skills.iter().enumerate() {
        let entry = &mut report.skills[index];
        if failure.is_some() {
            entry.result = Some(SkillResult::NotAttempted);
            continue;
        }
        let step = match entry.action {
            Action::Install => replace_skill(root, skill, false).map(|()| SkillResult::Installed),
            Action::Update => replace_skill(root, skill, true).map(|()| SkillResult::Updated),
            _ => Ok(SkillResult::Unchanged),
        }
        .and_then(|result| {
            if plan.manifest.set(skill) {
                plan.manifest.write(root)?;
            }
            Ok(result)
        });
        match step {
            Ok(result) => entry.result = Some(result),
            Err(error) => {
                entry.result = Some(SkillResult::Failed);
                failure = Some(format!("{}: {error}", skill.name));
            }
        }
    }
    report.error = failure;
    report
}

impl TargetPlan {
    /// Reports every skill as not attempted, for a target that failed before
    /// its first skill.
    fn unattempted(catalog: &Catalog, root: &Path) -> Vec<SkillReport> {
        catalog
            .skills
            .iter()
            .map(|skill| SkillReport {
                name: skill.name.clone(),
                kind: skill.kind,
                destination: root.join(&skill.name),
                action: Action::Install,
                conflict: None,
                result: Some(SkillResult::NotAttempted),
                requirements: skill.requirements.clone(),
            })
            .collect()
    }
}

/// Takes the skills directory's write lock until the returned file closes.
fn lock(root: &Path) -> Result<File, InstallError> {
    let path = root.join(LOCK_FILE);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o644)
        .open(&path)
        .map_err(|source| InstallError::Write {
            path: path.clone(),
            source,
        })?;
    file.lock()
        .map_err(|source| InstallError::Write { path, source })?;
    Ok(file)
}

/// Writes the bundled skill beside its destination, then renames it into
/// place. With `replace`, the existing directory is moved aside first and
/// restored if the new copy cannot take its place.
fn replace_skill(root: &Path, skill: &Skill, replace: bool) -> Result<(), InstallError> {
    let destination = root.join(&skill.name);
    let staging = root.join(format!(".leme-staging-{}", skill.name));
    let previous = root.join(format!(".leme-replaced-{}", skill.name));
    remove_leftover(&staging)?;
    remove_leftover(&previous)?;

    let written = write_staging(&staging, skill);
    if let Err(error) = written {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }

    if replace {
        fs::rename(&destination, &previous).map_err(|source| {
            let _ = fs::remove_dir_all(&staging);
            InstallError::Write {
                path: destination.clone(),
                source,
            }
        })?;
    }
    if let Err(source) = fs::rename(&staging, &destination) {
        if replace {
            let _ = fs::rename(&previous, &destination);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(InstallError::Write {
            path: destination,
            source,
        });
    }
    sync_directory(root)?;
    if replace {
        fs::remove_dir_all(&previous).map_err(|source| InstallError::Write {
            path: previous,
            source,
        })?;
    }
    Ok(())
}

/// Removes a staging directory left by an interrupted earlier run. Only
/// Leme creates these names, and the lock keeps other Leme runs out.
fn remove_leftover(path: &Path) -> Result<(), InstallError> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(InstallError::Write {
            path: path.to_path_buf(),
            source: error,
        }),
        _ => Ok(()),
    }
}

fn write_staging(staging: &Path, skill: &Skill) -> Result<(), InstallError> {
    let failed = |path: &Path| {
        let path = path.to_path_buf();
        move |source| InstallError::Write { path, source }
    };
    fs::create_dir(staging).map_err(failed(staging))?;
    for file in &skill.files {
        let path = staging.join(&file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(failed(parent))?;
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(&path)
            .map_err(failed(&path))?;
        output.write_all(file.contents).map_err(failed(&path))?;
        output.sync_all().map_err(failed(&path))?;
    }
    sync_directory(staging)
}

fn sync_directory(path: &Path) -> Result<(), InstallError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| InstallError::Write {
            path: path.to_path_buf(),
            source,
        })
}

/// `.leme-skills.toml`: the skills Leme installed in one skills directory,
/// with the hash of every file as installed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    #[serde(default)]
    skills: Vec<ManagedSkill>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagedSkill {
    name: String,
    files: Vec<ManagedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagedFile {
    path: String,
    sha256: String,
}

impl Manifest {
    /// Reads the manifest in `root`; a missing one is empty.
    fn read(root: &Path) -> Result<Self, InstallError> {
        let path = root.join(MANIFEST_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self {
                    schema_version: MANIFEST_SCHEMA_VERSION,
                    skills: Vec::new(),
                });
            }
            Err(source) => return Err(InstallError::Read { path, source }),
        };
        let manifest: Self = toml::from_str(&text).map_err(|error| InstallError::Manifest {
            path: path.clone(),
            detail: error.message().to_owned(),
        })?;
        if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(InstallError::Manifest {
                path,
                detail: format!(
                    "schema_version {} is not supported; this Leme reads version {MANIFEST_SCHEMA_VERSION}",
                    manifest.schema_version
                ),
            });
        }
        Ok(manifest)
    }

    /// The recorded contents of the Leme copy of `name`, if any.
    fn record(&self, name: &str) -> Option<Tree> {
        self.skills
            .iter()
            .find(|skill| skill.name == name)
            .map(|skill| {
                skill
                    .files
                    .iter()
                    .map(|file| (file.path.clone(), file.sha256.clone()))
                    .collect()
            })
    }

    /// Records `skill` as installed with its bundled contents. Returns
    /// whether the record changed.
    fn set(&mut self, skill: &Skill) -> bool {
        if self.record(&skill.name) == Some(bundled_tree(skill)) {
            return false;
        }
        let entry = ManagedSkill {
            name: skill.name.clone(),
            files: skill
                .files
                .iter()
                .map(|file| ManagedFile {
                    path: file.path.clone(),
                    sha256: file.sha256.clone(),
                })
                .collect(),
        };
        match self.skills.iter_mut().find(|s| s.name == skill.name) {
            Some(existing) => *existing = entry,
            None => self.skills.push(entry),
        }
        self.skills.sort_by(|a, b| a.name.cmp(&b.name));
        true
    }

    /// Replaces the manifest in `root` atomically.
    fn write(&self, root: &Path) -> Result<(), InstallError> {
        let path = root.join(MANIFEST_FILE);
        let temporary = root.join(format!("{MANIFEST_FILE}.tmp"));
        let failed = |path: &Path| {
            let path = path.to_path_buf();
            move |source| InstallError::Write { path, source }
        };
        let text = format!(
            "# Skills installed here by `leme skills install`, with the SHA-256 of each\n\
             # file as installed. Leme updates a skill only while it still matches.\n\n{}",
            toml::to_string(self).expect("the manifest always serializes")
        );
        let mut output = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o644)
            .open(&temporary)
            .map_err(failed(&temporary))?;
        output
            .write_all(text.as_bytes())
            .and_then(|()| output.sync_all())
            .map_err(failed(&temporary))?;
        fs::rename(&temporary, &path).map_err(failed(&path))?;
        sync_directory(root)
    }
}

/// A failure that stops installation before anything is written, or stops
/// one target while it is written.
#[derive(Debug)]
pub enum InstallError {
    /// `HOME` is not set.
    NoHome,
    /// A location variable holds a relative path.
    RelativeVariable {
        variable: &'static str,
        value: PathBuf,
    },
    /// The skills directory path exists but is not a directory.
    RootNotADirectory(PathBuf),
    /// A destination could not be read.
    Read { path: PathBuf, source: io::Error },
    /// The manifest is malformed.
    Manifest { path: PathBuf, detail: String },
    /// A file or directory could not be written.
    Write { path: PathBuf, source: io::Error },
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoHome => write!(f, "HOME is not set, so the skills directory is unknown"),
            Self::RelativeVariable { variable, value } => write!(
                f,
                "{variable} is the relative path {}; set it to an absolute path",
                value.display()
            ),
            Self::RootNotADirectory(path) => {
                write!(f, "{} exists but is not a directory", path.display())
            }
            Self::Read { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Self::Manifest { path, detail } => write!(
                f,
                "{} is not a valid Leme skills manifest ({detail}); move it aside and run again",
                path.display()
            ),
            Self::Write { path, source } => write!(f, "cannot write {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for InstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } | Self::Write { source, .. } => Some(source),
            _ => None,
        }
    }
}
