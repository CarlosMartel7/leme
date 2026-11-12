//! Embeds the bundled workflow skills in the executable.
//!
//! Every file under `agents/skills/<skill>/` becomes a `BundledFile` entry in
//! `$OUT_DIR/bundled_skills.rs`, and `agents/skills/catalog.toml` becomes
//! `CATALOG`. The build only collects bytes; `src/skills/catalog.rs` checks
//! them against the catalog's inventory and hashes before anything is
//! installed. A symbolic link or other special file fails the build, so a
//! bundled skill can never point outside the repository.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("set by Cargo"));
    let skills = manifest.join("agents").join("skills");
    println!("cargo:rerun-if-changed={}", skills.display());

    let mut files = Vec::new();
    for entry in sorted_entries(&skills) {
        let kind = entry
            .symlink_metadata()
            .map(|metadata| metadata.file_type());
        match kind {
            Ok(kind) if kind.is_dir() => {
                let skill = file_name(&entry);
                collect(&skill, &entry, Path::new(""), &mut files);
            }
            Ok(kind) if kind.is_file() => {} // catalog.toml, .gitattributes
            _ => panic!("{} is not a regular file or directory", entry.display()),
        }
    }

    let mut code = format!(
        "pub(crate) const CATALOG: &str = include_str!({:?});\n\n\
         pub(crate) const FILES: &[BundledFile] = &[\n",
        path_text(&skills.join("catalog.toml"))
    );
    for (skill, relative, absolute) in &files {
        writeln!(
            code,
            "    BundledFile {{ skill: {skill:?}, path: {relative:?}, contents: include_bytes!({:?}) }},",
            path_text(absolute)
        )
        .expect("writing to a String cannot fail");
    }
    code.push_str("];\n");

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("set by Cargo"));
    fs::write(out.join("bundled_skills.rs"), code).expect("cannot write bundled_skills.rs");
}

/// Adds every regular file below `directory` to `files`, with its path
/// relative to the skill directory, using `/` separators.
fn collect(
    skill: &str,
    directory: &Path,
    relative: &Path,
    files: &mut Vec<(String, String, PathBuf)>,
) {
    for entry in sorted_entries(directory) {
        let name = file_name(&entry);
        let inner = relative.join(&name);
        let kind = entry
            .symlink_metadata()
            .unwrap_or_else(|error| panic!("cannot inspect {}: {error}", entry.display()))
            .file_type();
        if kind.is_dir() {
            collect(skill, &entry, &inner, files);
        } else if kind.is_file() {
            files.push((
                skill.to_owned(),
                path_text(&inner).replace('\\', "/"),
                entry,
            ));
        } else {
            panic!(
                "{} is a symbolic link or special file; bundled skills may contain only regular files",
                entry.display()
            );
        }
    }
}

fn sorted_entries(directory: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", directory.display()))
        .map(|entry| entry.expect("cannot read directory entry").path())
        .collect();
    entries.sort();
    entries
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| panic!("{} has a name that is not UTF-8", path.display()))
        .to_owned()
}

fn path_text(path: &Path) -> String {
    path.to_str()
        .unwrap_or_else(|| panic!("{} is not a UTF-8 path", path.display()))
        .to_owned()
}
