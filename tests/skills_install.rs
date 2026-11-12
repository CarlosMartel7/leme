//! Runs `leme skills install` against disposable home directories, and the
//! catalog validation and installer against fake bundles.

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

use leme::skills::{
    self, Action, BundledFile, Catalog, CatalogError, Conflict, Environment, MANIFEST_FILE,
    Outcome, SkillResult, Target,
};
use support::skills::{
    Entry, SkillsHome, assert_installed_exactly, catalog_files, catalog_skills,
    catalog_source_paths, field_values, snapshot, target_skills,
};

const SKILL_COUNT: usize = 11;

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

// ---------------------------------------------------------------------------
// Installing for each target

#[test]
fn codex_target_installs_every_skill_exactly_and_only_for_codex() {
    let home = SkillsHome::new();
    let output = home.install(&["--target", "codex"]);
    assert!(output.status.success(), "{}", stderr(&output));

    assert_installed_exactly(&home.codex_root());
    assert!(!home.claude_root().exists(), "Claude Code was not selected");
    assert!(stdout(&output).contains(&home.codex_root().display().to_string()));
}

#[test]
fn claude_target_installs_every_skill_exactly_and_only_for_claude() {
    let home = SkillsHome::new();
    let output = home.install(&["--target", "claude"]);
    assert!(output.status.success(), "{}", stderr(&output));

    assert_installed_exactly(&home.claude_root());
    assert!(
        !home.home().join(".agents").exists(),
        "Codex was not selected"
    );
}

#[test]
fn both_targets_report_every_skill_as_installed() {
    let home = SkillsHome::new();
    let (code, report) = home.install_json(&["--target", "both"]);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["outcome"], "installed");
    assert_eq!(report["dry_run"], false);

    for (target, root) in [("codex", home.codex_root()), ("claude", home.claude_root())] {
        assert_installed_exactly(&root);
        let skills = target_skills(&report, target);
        assert_eq!(skills.len(), SKILL_COUNT);
        assert_eq!(field_values(&report, target, "result"), ["installed"]);
        for (name, skill) in skills {
            assert_eq!(skill["destination"], root.join(name).to_str().unwrap());
        }
    }
}

#[test]
fn the_target_is_required() {
    let home = SkillsHome::new();
    let output = home.install(&[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("--target"), "{}", stderr(&output));
    assert!(snapshot(&home.home()).is_empty());
}

#[test]
fn supporting_files_and_the_helper_dependency_are_installed() {
    let home = SkillsHome::new();
    let (code, report) = home.install_json(&["--target", "both"]);
    assert_eq!(code, Some(0), "{report}");

    for root in [home.codex_root(), home.claude_root()] {
        for file in [
            "domain-modeling/ADR-FORMAT.md",
            "domain-modeling/GLOSSARY-FORMAT.md",
            "grill-with-docs/agents/openai.yaml",
            "pr-audit/agents/openai.yaml",
        ] {
            assert!(
                root.join(file).is_file(),
                "{} missing",
                root.join(file).display()
            );
        }
        // grill-with-docs invokes these two by name.
        assert!(root.join("grilling/SKILL.md").is_file());
        assert!(root.join("domain-modeling/SKILL.md").is_file());
    }
    for target in ["codex", "claude"] {
        let skills = target_skills(&report, target);
        assert_eq!(skills["domain-modeling"]["kind"], "dependency");
        assert_eq!(skills["grill-with-docs"]["kind"], "requested");
    }
    let requested = catalog_skills()
        .into_iter()
        .filter(|(_, kind)| kind == "requested")
        .count();
    assert_eq!(requested, 10);
}

#[test]
fn line_endings_are_installed_byte_for_byte() {
    let home = SkillsHome::new();
    assert!(home.install(&["--target", "codex"]).status.success());
    let installed = fs::read(home.codex_root().join("plan-feature-commits/SKILL.md")).unwrap();
    assert!(
        installed.windows(2).any(|pair| pair == b"\r\n"),
        "CRLF kept"
    );
}

#[test]
fn claude_config_dir_moves_the_claude_destination() {
    let home = SkillsHome::new();
    let config = home.home().join("claude-work");
    let output = home
        .command(&["--target", "claude"])
        .env("CLAUDE_CONFIG_DIR", config.to_str().unwrap())
        .run();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_installed_exactly(&config.join("skills"));
    assert!(!home.claude_root().exists());
}

#[test]
fn a_relative_claude_config_dir_is_refused_without_writing() {
    let home = SkillsHome::new();
    let output = home
        .command(&["--target", "both"])
        .env("CLAUDE_CONFIG_DIR", "claude-work")
        .run();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("CLAUDE_CONFIG_DIR"),
        "{}",
        stderr(&output)
    );
    assert!(
        snapshot(&home.home()).is_empty(),
        "nothing written, Codex included"
    );
}

// ---------------------------------------------------------------------------
// Dry runs and reruns

#[test]
fn a_dry_run_writes_nothing_and_lists_the_destinations() {
    let home = SkillsHome::new();
    fs::create_dir_all(home.claude_root().join("unrelated")).unwrap();
    let before = snapshot(&home.home());

    let (code, report) = home.install_json(&["--target", "both", "--dry-run"]);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["outcome"], "planned");
    assert_eq!(report["dry_run"], true);
    assert_eq!(field_values(&report, "codex", "action"), ["install"]);
    assert_eq!(field_values(&report, "codex", "result"), ["null"]);
    assert_eq!(
        snapshot(&home.home()),
        before,
        "the dry run changed the home"
    );

    let text = home.install(&["--target", "both", "--dry-run"]);
    assert!(text.status.success());
    assert!(stdout(&text).contains("would install"));
    assert!(stdout(&text).contains("nothing was written"));
    assert_eq!(snapshot(&home.home()), before);
}

#[test]
fn a_dry_run_after_installing_reports_everything_unchanged() {
    let home = SkillsHome::new();
    assert!(home.install(&["--target", "both"]).status.success());
    let before = snapshot(&home.home());

    let (code, report) = home.install_json(&["--target", "both", "--dry-run"]);
    assert_eq!(code, Some(0));
    assert_eq!(field_values(&report, "codex", "action"), ["unchanged"]);
    assert_eq!(field_values(&report, "claude", "action"), ["unchanged"]);
    assert_eq!(snapshot(&home.home()), before);
}

#[test]
fn an_identical_rerun_rewrites_nothing() {
    let home = SkillsHome::new();
    assert!(home.install(&["--target", "both"]).status.success());
    let before = snapshot(&home.home());

    let (code, report) = home.install_json(&["--target", "both"]);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["outcome"], "installed");
    assert_eq!(field_values(&report, "codex", "result"), ["unchanged"]);
    assert_eq!(field_values(&report, "claude", "result"), ["unchanged"]);
    // Same inodes and modification times: no file, manifest included, was
    // rewritten. The lock file is created once and reused.
    assert_eq!(snapshot(&home.home()), before);
}

#[test]
fn an_existing_identical_copy_is_adopted_and_recorded() {
    let home = SkillsHome::new();
    let root = home.codex_root();
    copy_bundled_skill("grilling", &root.join("grilling"));
    let original = snapshot(&root.join("grilling"));

    let (code, report) = home.install_json(&["--target", "codex"]);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(
        target_skills(&report, "codex")["grilling"]["result"],
        "unchanged"
    );
    assert_eq!(snapshot(&root.join("grilling")), original, "left as it was");
    let manifest = fs::read_to_string(root.join(MANIFEST_FILE)).unwrap();
    assert!(manifest.contains("name = \"grilling\""), "{manifest}");
}

#[test]
fn an_unchanged_older_leme_copy_is_updated() {
    let home = SkillsHome::new();
    let root = home.codex_root();
    assert!(home.install(&["--target", "codex"]).status.success());

    // Make the installed copy look like an earlier bundle: change a file and
    // record the changed hash, as an older Leme would have.
    let skill_file = root.join("grilling/SKILL.md");
    let bundled_hash = support::skills::sha256_hex(&fs::read(&skill_file).unwrap());
    fs::write(&skill_file, "older bundled text\n").unwrap();
    let older_hash = support::skills::sha256_hex(b"older bundled text\n");
    let manifest_path = root.join(MANIFEST_FILE);
    let manifest = fs::read_to_string(&manifest_path).unwrap();
    assert!(manifest.contains(&bundled_hash));
    fs::write(&manifest_path, manifest.replace(&bundled_hash, &older_hash)).unwrap();

    let (code, report) = home.install_json(&["--target", "codex"]);
    assert_eq!(code, Some(0), "{report}");
    let skills = target_skills(&report, "codex");
    assert_eq!(skills["grilling"]["action"], "update");
    assert_eq!(skills["grilling"]["result"], "updated");
    assert_eq!(skills["pr-audit"]["result"], "unchanged");
    assert_installed_exactly(&root);
    assert!(
        fs::read_to_string(&manifest_path)
            .unwrap()
            .contains(&bundled_hash)
    );
    assert_no_staging_left(&root);
}

// ---------------------------------------------------------------------------
// Conflicts and unrelated files

#[test]
fn unrelated_skills_and_files_are_preserved() {
    let home = SkillsHome::new();
    for root in [home.codex_root(), home.claude_root()] {
        fs::create_dir_all(root.join("my-skill/notes")).unwrap();
        fs::write(root.join("my-skill/SKILL.md"), "mine\n").unwrap();
        fs::write(root.join("my-skill/notes/a.md"), "a\n").unwrap();
        fs::write(root.join("README.txt"), "loose file\n").unwrap();
    }
    let unrelated: Vec<_> = [home.codex_root(), home.claude_root()]
        .iter()
        .map(|root| {
            (
                snapshot(&root.join("my-skill")),
                fs::read(root.join("README.txt")).unwrap(),
            )
        })
        .collect();

    let output = home.install(&["--target", "both"]);
    assert!(output.status.success(), "{}", stderr(&output));

    for (root, (skill, loose)) in [home.codex_root(), home.claude_root()]
        .iter()
        .zip(unrelated)
    {
        assert_eq!(snapshot(&root.join("my-skill")), skill);
        assert_eq!(fs::read(root.join("README.txt")).unwrap(), loose);
        assert_installed_exactly(root);
        let manifest = fs::read_to_string(root.join(MANIFEST_FILE)).unwrap();
        assert!(
            !manifest.contains("my-skill"),
            "unrelated skills are not claimed"
        );
    }
}

#[test]
fn an_unmanaged_collision_stops_every_target_before_writing() {
    let home = SkillsHome::new();
    let collision = home.claude_root().join("pr-audit");
    fs::create_dir_all(&collision).unwrap();
    fs::write(collision.join("SKILL.md"), "my own pr-audit\n").unwrap();
    let before = snapshot(&home.home());

    let (code, report) = home.install_json(&["--target", "both"]);
    assert_eq!(code, Some(1), "{report}");
    assert_eq!(report["outcome"], "conflict");
    let claude = target_skills(&report, "claude");
    assert_eq!(claude["pr-audit"]["action"], "conflict");
    assert_eq!(claude["pr-audit"]["conflict"], "unmanaged");
    assert_eq!(claude["grilling"]["action"], "install");
    assert_eq!(field_values(&report, "codex", "result"), ["null"]);
    assert_eq!(
        snapshot(&home.home()),
        before,
        "Codex was not written either"
    );

    let text = home.install(&["--target", "both"]);
    assert_eq!(text.status.code(), Some(1));
    assert!(
        stdout(&text).contains("did not install"),
        "{}",
        stdout(&text)
    );
    assert!(stderr(&text).contains("conflict"), "{}", stderr(&text));
}

#[test]
fn a_locally_modified_leme_copy_is_refused() {
    let home = SkillsHome::new();
    assert!(home.install(&["--target", "both"]).status.success());
    let edited = home.codex_root().join("for-dummies/SKILL.md");
    let mut text = fs::read(&edited).unwrap();
    text.extend_from_slice(b"\nMy local rule.\n");
    fs::write(&edited, &text).unwrap();
    let before = snapshot(&home.home());

    let (code, report) = home.install_json(&["--target", "both"]);
    assert_eq!(code, Some(1));
    let codex = target_skills(&report, "codex");
    assert_eq!(codex["for-dummies"]["conflict"], "locally_modified");
    assert_eq!(snapshot(&home.home()), before);
    assert_eq!(fs::read(&edited).unwrap(), text, "the local edit survives");
}

#[test]
fn an_added_file_in_a_leme_copy_is_a_local_modification() {
    let home = SkillsHome::new();
    assert!(home.install(&["--target", "codex"]).status.success());
    fs::write(home.codex_root().join("grilling/extra.md"), "added\n").unwrap();

    let (code, report) = home.install_json(&["--target", "codex"]);
    assert_eq!(code, Some(1));
    assert_eq!(
        target_skills(&report, "codex")["grilling"]["conflict"],
        "locally_modified"
    );
}

#[test]
fn a_symlinked_skill_directory_is_a_conflict_and_is_not_followed() {
    let home = SkillsHome::new();
    let elsewhere = home.home().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("SKILL.md"), "outside\n").unwrap();
    fs::create_dir_all(home.codex_root()).unwrap();
    symlink(&elsewhere, home.codex_root().join("grilling")).unwrap();
    let before = snapshot(&home.home());

    let (code, report) = home.install_json(&["--target", "codex"]);
    assert_eq!(code, Some(1));
    assert_eq!(
        target_skills(&report, "codex")["grilling"]["conflict"],
        "not_a_directory"
    );
    assert_eq!(snapshot(&home.home()), before);
}

#[test]
fn a_symlink_inside_an_existing_skill_is_a_conflict() {
    let home = SkillsHome::new();
    assert!(home.install(&["--target", "codex"]).status.success());
    let secret = home.home().join("secret.txt");
    fs::write(&secret, "do not read\n").unwrap();
    symlink(&secret, home.codex_root().join("grilling/link.md")).unwrap();

    let (code, report) = home.install_json(&["--target", "codex"]);
    assert_eq!(code, Some(1));
    assert_eq!(
        target_skills(&report, "codex")["grilling"]["conflict"],
        "unsupported_entry"
    );
    assert_eq!(fs::read_to_string(&secret).unwrap(), "do not read\n");
}

#[test]
fn a_file_where_the_skill_directory_belongs_is_a_conflict() {
    let home = SkillsHome::new();
    fs::create_dir_all(home.claude_root()).unwrap();
    fs::write(home.claude_root().join("grilling"), "a file\n").unwrap();

    let (code, report) = home.install_json(&["--target", "claude"]);
    assert_eq!(code, Some(1));
    assert_eq!(
        target_skills(&report, "claude")["grilling"]["conflict"],
        "not_a_directory"
    );
}

#[test]
fn an_invalid_manifest_stops_before_writing() {
    let home = SkillsHome::new();
    fs::create_dir_all(home.codex_root()).unwrap();
    fs::write(home.codex_root().join(MANIFEST_FILE), "not = [valid").unwrap();
    let before = snapshot(&home.home());

    let output = home.install(&["--target", "both"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains(MANIFEST_FILE),
        "{}",
        stderr(&output)
    );
    assert_eq!(snapshot(&home.home()), before);
}

// ---------------------------------------------------------------------------
// Failures while writing

#[test]
fn a_read_only_target_fails_alone_and_the_other_target_completes() {
    if running_as_root() {
        eprintln!("skipped: root ignores directory permissions");
        return;
    }
    let home = SkillsHome::new();
    fs::create_dir_all(home.claude_root()).unwrap();
    fs::set_permissions(home.claude_root(), fs::Permissions::from_mode(0o555)).unwrap();

    let (code, report) = home.install_json(&["--target", "both"]);
    fs::set_permissions(home.claude_root(), fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(code, Some(1), "{report}");
    assert_eq!(report["outcome"], "partial");
    assert_installed_exactly(&home.codex_root());
    assert_eq!(field_values(&report, "codex", "result"), ["installed"]);
    let claude = report["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["target"] == "claude")
        .unwrap();
    assert!(claude["error"].as_str().is_some(), "{claude}");
    assert!(
        !field_values(&report, "claude", "result").contains(&"installed".to_owned()),
        "{report}"
    );
    assert_eq!(fs::read_dir(home.claude_root()).unwrap().count(), 0);
}

#[test]
fn a_read_only_only_target_fails() {
    if running_as_root() {
        eprintln!("skipped: root ignores directory permissions");
        return;
    }
    let home = SkillsHome::new();
    fs::create_dir_all(home.codex_root()).unwrap();
    fs::set_permissions(home.codex_root(), fs::Permissions::from_mode(0o555)).unwrap();
    let (code, report) = home.install_json(&["--target", "codex"]);
    fs::set_permissions(home.codex_root(), fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(code, Some(1));
    assert_eq!(report["outcome"], "failed");
}

#[test]
fn two_concurrent_installs_leave_one_correct_copy() {
    let home = SkillsHome::new();
    let commands: Vec<Command> = (0..2)
        .map(|_| home.command(&["--target", "both"]).into_command())
        .collect();
    let outputs: Vec<_> = commands
        .into_iter()
        .map(|mut command| thread::spawn(move || command.output().expect("run leme")))
        .collect::<Vec<_>>()
        .into_iter()
        .map(|handle| handle.join().expect("thread"))
        .collect();

    for output in &outputs {
        assert!(output.status.success(), "{}", stderr(output));
    }
    for root in [home.codex_root(), home.claude_root()] {
        assert_installed_exactly(&root);
        assert_no_staging_left(&root);
    }
}

// ---------------------------------------------------------------------------
// Independence from the author's machine and the user's settings

#[test]
fn installation_needs_no_source_directory_and_leaves_agent_settings_alone() {
    let home = SkillsHome::new();
    // None of the catalog's source directories exist in this home.
    for source in catalog_source_paths() {
        let relative = source.strip_prefix("~/").expect("home-relative source");
        assert!(!home.home().join(relative).exists());
    }
    let settings = [
        (
            ".claude/settings.json",
            "{\"permissions\":{\"allow\":[]}}\n",
            0o600,
        ),
        (".claude.json", "{\"mcpServers\":{}}\n", 0o600),
        (".codex/config.toml", "model = \"x\"\n", 0o644),
        (".codex/auth.json", "{\"token\":\"secret\"}\n", 0o600),
    ];
    for (path, text, mode) in settings {
        let path = home.home().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
    }
    let before = snapshot(&home.home());

    let output = home.install(&["--target", "both"]);
    assert!(output.status.success(), "{}", stderr(&output));

    // Every file outside the two skills directories is exactly as before.
    let files_outside = |entries: BTreeMap<PathBuf, Entry>| {
        entries
            .into_iter()
            .filter(|(path, entry)| {
                !path.starts_with(".agents/skills")
                    && !path.starts_with(".claude/skills")
                    && !matches!(entry, Entry::Directory { .. })
            })
            .collect::<BTreeMap<_, _>>()
    };
    assert_eq!(
        files_outside(snapshot(&home.home())),
        files_outside(before),
        "settings, authentication and MCP files are untouched"
    );
}

#[test]
fn installing_never_runs_skill_content() {
    let marker_dir = support::TestDir::new();
    let marker = marker_dir.path().join("ran");
    let script = leak(format!("#!/bin/sh\ntouch '{}'\n", marker.to_str().unwrap()));
    let skill_md: &'static str = "---\nname: runner\n---\nRun ./run.sh first.\n";
    let catalog = fake_catalog(&[
        ("runner", "SKILL.md", skill_md),
        ("runner", "run.sh", script),
    ]);
    let home = support::TestDir::new();
    let environment = Environment {
        home: Some(home.path().as_os_str().to_owned()),
        claude_config_dir: None,
    };

    let report = skills::install(
        &catalog,
        &[Target::Codex, Target::Claude],
        &environment,
        false,
    )
    .expect("install");
    assert_eq!(report.outcome, Outcome::Installed);
    for root in [".agents/skills", ".claude/skills"] {
        let installed = home.path().join(root).join("runner/run.sh");
        assert_eq!(fs::read_to_string(&installed).unwrap(), script);
        let mode = fs::metadata(&installed).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644, "never made executable");
    }
    assert!(!marker.exists(), "the bundled script was executed");
}

// ---------------------------------------------------------------------------
// Catalog validation

#[test]
fn the_bundled_catalog_is_valid_and_matches_the_repository() {
    let catalog = Catalog::bundled().expect("bundled catalog");
    assert_eq!(catalog.skills.len(), SKILL_COUNT);
    let embedded: usize = catalog.skills.iter().map(|skill| skill.files.len()).sum();
    assert_eq!(embedded, catalog_files().len());
    let names: Vec<_> = catalog
        .skills
        .iter()
        .map(|skill| skill.name.clone())
        .collect();
    let expected: Vec<_> = catalog_skills().into_iter().map(|(name, _)| name).collect();
    assert_eq!(names, expected);
    let grill = catalog
        .skills
        .iter()
        .find(|skill| skill.name == "grill-with-docs")
        .unwrap();
    assert_eq!(grill.depends_on, ["grilling", "domain-modeling"]);
}

#[test]
fn a_catalog_file_missing_from_the_bundle_is_refused() {
    let text = catalog_text("runner", &[("SKILL.md", SKILL), ("extra.md", "x")]);
    let error = Catalog::new(&text, &files(&[("runner", "SKILL.md", SKILL)])).unwrap_err();
    assert!(matches!(error, CatalogError::Mismatch(ref detail) if detail.contains("extra.md")));
}

#[test]
fn a_bundled_file_missing_from_the_catalog_is_refused() {
    let text = catalog_text("runner", &[("SKILL.md", SKILL)]);
    let bundle = files(&[("runner", "SKILL.md", SKILL), ("runner", "stray.md", "x")]);
    let error = Catalog::new(&text, &bundle).unwrap_err();
    assert!(matches!(error, CatalogError::Mismatch(ref detail) if detail.contains("stray.md")));
}

#[test]
fn a_changed_file_is_refused() {
    let text = catalog_text("runner", &[("SKILL.md", SKILL)]);
    let changed: &'static str = "---\nname: runner\n---\nChanged.\n";
    let error = Catalog::new(&text, &files(&[("runner", "SKILL.md", changed)])).unwrap_err();
    assert!(matches!(error, CatalogError::Mismatch(_)), "{error}");
    assert!(error.to_string().contains("nothing was installed"));
}

#[test]
fn unsafe_catalog_paths_are_refused() {
    for path in [
        "../escape.md",
        "/etc/passwd",
        "a//b.md",
        "./SKILL.md",
        "a\\b.md",
        "",
    ] {
        let text = catalog_text("runner", &[("SKILL.md", SKILL), (path, "x")]);
        let error = Catalog::new(
            &text,
            &files(&[
                ("runner", "SKILL.md", SKILL),
                ("runner", leak(path.to_owned()), "x"),
            ]),
        )
        .unwrap_err();
        assert!(
            matches!(error, CatalogError::Invalid(_)),
            "{path:?}: {error}"
        );
    }
}

#[test]
fn unsafe_skill_names_are_refused() {
    for name in ["../up", "Upper", "-dash", "with space", ""] {
        let text = catalog_text(name, &[("SKILL.md", SKILL)]);
        let error =
            Catalog::new(&text, &files(&[(leak(name.to_owned()), "SKILL.md", SKILL)])).unwrap_err();
        assert!(
            matches!(error, CatalogError::Invalid(_)),
            "{name:?}: {error}"
        );
    }
}

#[test]
fn a_dependency_that_is_not_bundled_is_refused() {
    let text = catalog_text("runner", &[("SKILL.md", SKILL)])
        .replace("depends_on = []", "depends_on = [\"domain-modeling\"]");
    let error = Catalog::new(&text, &files(&[("runner", "SKILL.md", SKILL)])).unwrap_err();
    assert!(error.to_string().contains("domain-modeling"), "{error}");
}

#[test]
fn a_skill_without_skill_md_is_refused() {
    let text = catalog_text("runner", &[("README.md", "x")]);
    let error = Catalog::new(&text, &files(&[("runner", "README.md", "x")])).unwrap_err();
    assert!(error.to_string().contains("SKILL.md"), "{error}");
}

#[test]
fn the_planned_actions_are_exposed_by_the_library() {
    // The command's JSON names come from these types; keep them stable.
    let names = [
        (serde_json::to_value(Action::Install).unwrap(), "install"),
        (serde_json::to_value(Action::Update).unwrap(), "update"),
        (
            serde_json::to_value(Conflict::LocallyModified).unwrap(),
            "locally_modified",
        ),
        (
            serde_json::to_value(SkillResult::NotAttempted).unwrap(),
            "not_attempted",
        ),
        (serde_json::to_value(Outcome::Partial).unwrap(), "partial"),
    ];
    for (value, expected) in names {
        assert_eq!(value, expected);
    }
}

// ---------------------------------------------------------------------------
// Helpers

const SKILL: &str = "---\nname: runner\n---\nA test skill.\n";

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

fn files(entries: &[(&'static str, &'static str, &'static str)]) -> Vec<BundledFile> {
    entries
        .iter()
        .map(|&(skill, path, contents)| BundledFile {
            skill,
            path,
            contents: contents.as_bytes(),
        })
        .collect()
}

/// A one-skill catalog listing `entries` with their real sizes and hashes.
fn catalog_text(name: &str, entries: &[(&str, &str)]) -> String {
    let mut text = format!(
        "schema_version = 1\nsnapshot_date = \"2026-11-12\"\n\n[[skills]]\n\
         name = {name:?}\nkind = \"requested\"\nsource_path = \"~/skills/{name}\"\n\
         source_name = {name:?}\ndepends_on = []\nreferences = []\nrequirements = []\n"
    );
    for (path, contents) in entries {
        text.push_str(&format!(
            "\n[[skills.files]]\npath = {path:?}\nsize = {}\nsha256 = \"{}\"\n",
            contents.len(),
            support::skills::sha256_hex(contents.as_bytes())
        ));
    }
    text
}

fn fake_catalog(entries: &[(&'static str, &'static str, &'static str)]) -> Catalog {
    let name = entries[0].0;
    let listed: Vec<(&str, &str)> = entries
        .iter()
        .map(|&(_, path, text)| (path, text))
        .collect();
    Catalog::new(&catalog_text(name, &listed), &files(entries)).expect("fake catalog")
}

fn copy_bundled_skill(name: &str, destination: &Path) {
    for file in catalog_files()
        .into_iter()
        .filter(|file| file.skill == name)
    {
        let target = destination.join(&file.path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(
            support::skills::bundle_directory()
                .join(name)
                .join(&file.path),
            &target,
        )
        .unwrap();
    }
}

fn assert_no_staging_left(root: &Path) {
    let leftovers: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".leme-staging-") || name.starts_with(".leme-replaced-"))
        .collect();
    assert!(leftovers.is_empty(), "left behind: {leftovers:?}");
}

fn running_as_root() -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() == 0 }
}
