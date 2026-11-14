//! Resolves project preferences from TOML values and from `leme.toml`.

mod support;

use std::fs;

use leme::config::{Agent, ClockFormat, ConfigError, FILE_NAME, Preferences};
use support::{Leme, TestDir, add_worktree, failure_message, init_repository, status_json};

fn parse_error(text: &str) -> String {
    Preferences::parse(text)
        .expect_err("invalid preferences")
        .to_string()
}

#[test]
fn an_empty_file_uses_the_defaults() {
    assert_eq!(
        Preferences::parse("").unwrap(),
        Preferences {
            integration_branch: "main".to_owned(),
            preferred_agent: Agent::Claude,
            clock_format: ClockFormat::TwentyFourHour,
        }
    );
}

#[test]
fn explicit_values_replace_the_defaults() {
    let preferences = Preferences::parse(
        r#"
        integration_branch = "develop"
        preferred_agent = "codex"
        clock_format = "12h"
        "#,
    )
    .unwrap();

    assert_eq!(
        preferences,
        Preferences {
            integration_branch: "develop".to_owned(),
            preferred_agent: Agent::Codex,
            clock_format: ClockFormat::TwelveHour,
        }
    );
}

#[test]
fn each_setting_is_optional() {
    let branch = Preferences::parse(r#"integration_branch = "develop""#).unwrap();
    assert_eq!(branch.integration_branch, "develop");
    assert_eq!(branch.preferred_agent, Agent::Claude);

    let agent = Preferences::parse(r#"preferred_agent = "codex""#).unwrap();
    assert_eq!(agent.integration_branch, "main");
    assert_eq!(agent.preferred_agent, Agent::Codex);
}

#[test]
fn malformed_toml_is_rejected_at_its_line() {
    let error = parse_error("integration_branch = \"develop\"\npreferred_agent = \n");

    assert!(error.contains("line 2"), "{error}");
    assert!(error.contains("preferred_agent"), "{error}");
}

#[test]
fn an_unsupported_agent_lists_the_supported_ones() {
    let error = parse_error(r#"preferred_agent = "gemini""#);

    assert!(error.contains("preferred_agent"), "{error}");
    assert!(error.contains("gemini"), "{error}");
    assert!(error.contains("`claude` or `codex`"), "{error}");
}

#[test]
fn agent_names_are_case_sensitive() {
    let error = parse_error(r#"preferred_agent = "Codex""#);

    assert!(error.contains("preferred_agent"), "{error}");
}

#[test]
fn an_unsupported_clock_format_lists_the_supported_ones() {
    let error = parse_error(r#"clock_format = "24""#);

    assert!(error.contains("clock_format"), "{error}");
    assert!(error.contains("`24h` or `12h`"), "{error}");
}

#[test]
fn a_value_of_the_wrong_type_is_rejected() {
    let error = parse_error("integration_branch = 3");

    assert!(error.contains("integration_branch"), "{error}");
    assert!(error.contains("expected a string"), "{error}");
}

#[test]
fn a_blank_branch_name_is_rejected() {
    for text in [r#"integration_branch = """#, r#"integration_branch = "  ""#] {
        let error = parse_error(text);

        assert!(error.contains("integration_branch"), "{error}");
        assert!(error.contains("must not be empty"), "{error}");
    }
}

#[test]
fn an_unknown_setting_is_rejected() {
    let error = parse_error(r#"preferred_agnet = "codex""#);

    assert!(error.contains("unknown field `preferred_agnet`"), "{error}");
}

#[test]
fn a_missing_file_uses_the_defaults() {
    let root = TestDir::new();

    assert_eq!(
        Preferences::load(root.path()).unwrap(),
        Preferences::default()
    );
}

#[test]
fn an_invalid_file_is_named_with_its_offending_field() {
    let root = TestDir::new();
    let path = root.path().join(FILE_NAME);
    fs::write(&path, "preferred_agent = \"gemini\"\n").unwrap();

    let error = Preferences::load(root.path()).unwrap_err();

    assert!(matches!(error, ConfigError::Invalid { .. }), "{error:?}");
    let message = error.to_string();
    assert!(message.contains(path.to_str().unwrap()), "{message}");
    assert!(message.contains("preferred_agent"), "{message}");
    assert_eq!(message, message.trim_end());
}

#[test]
fn an_unreadable_file_is_not_treated_as_missing() {
    let root = TestDir::new();
    let path = root.path().join(FILE_NAME);
    fs::create_dir(&path).unwrap();

    let error = Preferences::load(root.path()).unwrap_err();

    assert!(matches!(error, ConfigError::Unreadable { .. }), "{error:?}");
    assert!(error.to_string().contains(path.to_str().unwrap()));
}

#[test]
fn a_linked_worktree_reads_the_main_checkout() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    let worktree = root.path().join("feature");
    init_repository(&repository);
    add_worktree(&repository, &worktree, "feature/one");
    fs::write(
        repository.join(FILE_NAME),
        "integration_branch = \"develop\"\n",
    )
    .unwrap();
    fs::write(
        worktree.join(FILE_NAME),
        "integration_branch = \"ignored\"\n",
    )
    .unwrap();

    let status = status_json(&worktree, root.path());

    assert_eq!(status["preferences"]["integration_branch"], "develop");
}

#[test]
fn status_fails_on_an_invalid_file() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);
    fs::write(repository.join(FILE_NAME), "integration_branch = 3\n").unwrap();

    let output = Leme::new(&repository, root.path(), &["status"]).run();

    let message = failure_message(&output);
    assert!(
        message.contains(repository.join(FILE_NAME).to_str().unwrap()),
        "{message}"
    );
    assert!(message.contains("integration_branch"), "{message}");
}
