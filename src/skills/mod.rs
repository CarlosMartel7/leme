//! The workflow skills bundled with Leme and their installer.
//!
//! `catalog` validates the skills embedded from `agents/skills/`; `install`
//! copies them into Codex's or Claude Code's user skills directory.

pub mod catalog;
pub mod install;

pub use catalog::{BundledFile, Catalog, CatalogError, Skill, SkillKind};
pub use install::{
    Action, Conflict, Environment, InstallError, MANIFEST_FILE, Outcome, Report, SkillReport,
    SkillResult, Target, TargetReport, install, skills_root,
};
