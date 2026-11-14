//! Fixtures for `leme skills install`: disposable homes, the repository's
//! skill catalog, and snapshots of directory trees.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Output;

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{Leme, TestDir};

/// A disposable home directory for one test.
pub struct SkillsHome {
    dir: TestDir,
}

impl SkillsHome {
    pub fn new() -> Self {
        let dir = TestDir::new();
        fs::create_dir(dir.path().join("home")).expect("create home");
        Self { dir }
    }

    /// The directory the test uses as `HOME`.
    pub fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    /// Codex's user skills directory in this home.
    pub fn codex_root(&self) -> PathBuf {
        self.home().join(".agents/skills")
    }

    /// Claude Code's user skills directory in this home.
    pub fn claude_root(&self) -> PathBuf {
        self.home().join(".claude/skills")
    }

    /// Prepares `leme skills install <arguments>` with this home and no
    /// `CLAUDE_CONFIG_DIR`, run from outside any repository.
    pub fn command(&self, arguments: &[&str]) -> Leme {
        let mut full = vec!["skills", "install"];
        full.extend_from_slice(arguments);
        Leme::new(self.dir.path(), self.dir.path(), &full)
            .env("HOME", self.home().to_str().expect("UTF-8 home"))
            .env_remove("CLAUDE_CONFIG_DIR")
    }

    /// Runs `leme skills install <arguments>`.
    pub fn install(&self, arguments: &[&str]) -> Output {
        self.command(arguments).run()
    }

    /// Runs `leme skills install <arguments> --json` and returns the exit
    /// code with the parsed report.
    pub fn install_json(&self, arguments: &[&str]) -> (Option<i32>, Value) {
        let mut full = arguments.to_vec();
        full.push("--json");
        let output = self.install(&full);
        let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "report is not JSON ({error}); stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.code(), report)
    }
}

/// One file of the repository's catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogFile {
    pub skill: String,
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Deserialize)]
struct CatalogText {
    skills: Vec<CatalogSkill>,
}

#[derive(Deserialize)]
struct CatalogSkill {
    name: String,
    kind: String,
    source_path: String,
    files: Vec<CatalogFileText>,
}

#[derive(Deserialize)]
struct CatalogFileText {
    path: String,
    size: u64,
    sha256: String,
}

/// The repository's `agents/skills` directory.
pub fn bundle_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("agents/skills")
}

fn catalog() -> CatalogText {
    let text = fs::read_to_string(bundle_directory().join("catalog.toml")).expect("read catalog");
    toml::from_str(&text).expect("parse catalog")
}

/// Every file the repository's catalog lists.
pub fn catalog_files() -> Vec<CatalogFile> {
    catalog()
        .skills
        .into_iter()
        .flat_map(|skill| {
            let name = skill.name;
            skill.files.into_iter().map(move |file| CatalogFile {
                skill: name.clone(),
                path: file.path,
                size: file.size,
                sha256: file.sha256,
            })
        })
        .collect()
}

/// The catalog's skill names, with their kind.
pub fn catalog_skills() -> Vec<(String, String)> {
    catalog()
        .skills
        .into_iter()
        .map(|skill| (skill.name, skill.kind))
        .collect()
}

/// The source paths the catalog records, as written (`~/...`).
pub fn catalog_source_paths() -> Vec<String> {
    catalog()
        .skills
        .into_iter()
        .map(|skill| skill.source_path)
        .collect()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// What a snapshot records for one entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Directory {
        mode: u32,
    },
    File {
        mode: u32,
        sha256: String,
        inode: u64,
        modified: i64,
    },
    Link {
        target: PathBuf,
    },
}

/// Every entry below `root` (not following links), keyed by relative path.
/// Recording inodes and modification times shows whether a file was
/// rewritten, not only whether its bytes changed.
pub fn snapshot(root: &Path) -> BTreeMap<PathBuf, Entry> {
    let mut entries = BTreeMap::new();
    if fs::symlink_metadata(root).is_ok() {
        walk(root, root, &mut entries);
    }
    entries
}

fn walk(root: &Path, directory: &Path, entries: &mut BTreeMap<PathBuf, Entry>) {
    for entry in fs::read_dir(directory).expect("read directory") {
        let path = entry.expect("directory entry").path();
        let metadata = fs::symlink_metadata(&path).expect("inspect entry");
        let relative = path.strip_prefix(root).expect("inside root").to_path_buf();
        let mode = metadata.permissions().mode() & 0o7777;
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path).expect("read link");
            entries.insert(relative, Entry::Link { target });
        } else if metadata.is_dir() {
            entries.insert(relative, Entry::Directory { mode });
            walk(root, &path, entries);
        } else {
            let bytes = fs::read(&path).expect("read file");
            entries.insert(
                relative,
                Entry::File {
                    mode,
                    sha256: sha256_hex(&bytes),
                    inode: metadata.ino(),
                    modified: metadata.mtime_nsec() + metadata.mtime() * 1_000_000_000,
                },
            );
        }
    }
}

/// Checks that `root` holds every catalog skill exactly as bundled: the
/// same files, sizes, hashes and bytes as the repository, mode 644, and no
/// other file in a skill directory.
pub fn assert_installed_exactly(root: &Path) {
    let files = catalog_files();
    for (skill, _) in catalog_skills() {
        let expected: Vec<&str> = files
            .iter()
            .filter(|file| file.skill == skill)
            .map(|file| file.path.as_str())
            .collect();
        let found = snapshot(&root.join(&skill));
        let mut found_files: Vec<String> = found
            .iter()
            .filter(|(_, entry)| !matches!(entry, Entry::Directory { .. }))
            .map(|(path, _)| path.to_str().expect("UTF-8 path").to_owned())
            .collect();
        found_files.sort();
        let mut expected_sorted: Vec<String> = expected.iter().map(|p| (*p).to_owned()).collect();
        expected_sorted.sort();
        assert_eq!(
            found_files,
            expected_sorted,
            "files installed for {skill} in {}",
            root.display()
        );
    }
    for file in &files {
        let installed = root.join(&file.skill).join(&file.path);
        let bytes = fs::read(&installed)
            .unwrap_or_else(|error| panic!("read {}: {error}", installed.display()));
        let bundled = fs::read(bundle_directory().join(&file.skill).join(&file.path))
            .expect("read bundled file");
        assert_eq!(
            bytes,
            bundled,
            "{} differs from the bundle",
            installed.display()
        );
        assert_eq!(
            bytes.len() as u64,
            file.size,
            "{} size",
            installed.display()
        );
        assert_eq!(
            sha256_hex(&bytes),
            file.sha256,
            "{} hash",
            installed.display()
        );
        let mode = fs::metadata(&installed).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o644, "{} mode", installed.display());
    }
}

/// The skills of one target in a JSON report, by name.
pub fn target_skills<'a>(report: &'a Value, target: &str) -> BTreeMap<&'a str, &'a Value> {
    report["targets"]
        .as_array()
        .expect("targets")
        .iter()
        .find(|entry| entry["target"] == target)
        .unwrap_or_else(|| panic!("no {target} target in {report}"))["skills"]
        .as_array()
        .expect("skills")
        .iter()
        .map(|skill| (skill["name"].as_str().expect("name"), skill))
        .collect()
}

/// Every distinct value of `field` among one target's skills.
pub fn field_values(report: &Value, target: &str, field: &str) -> Vec<String> {
    let mut values: Vec<String> = target_skills(report, target)
        .values()
        .map(|skill| skill[field].as_str().unwrap_or("null").to_owned())
        .collect();
    values.sort();
    values.dedup();
    values
}
