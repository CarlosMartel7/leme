//! Runs `leme status` against real Git layouts.

mod support;

use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use support::{
    Leme, TestDir, add_worktree, commit, failure_message, git, git_output, init_repository,
    status_json,
};

fn expected(repository: &Path, branch: Value, checkout: &Path) -> Value {
    json!({
        "repository": repository,
        "branch": branch,
        "checkout": checkout,
        "preferences": {
            "integration_branch": "main",
            "preferred_agent": "claude",
        },
    })
}

#[test]
fn reports_the_main_checkout() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);

    assert_eq!(
        status_json(&repository, root.path()),
        expected(&repository, json!("main"), &repository)
    );
}

#[test]
fn resolves_a_nested_directory_to_its_checkout() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);
    let nested = repository.join("src/deep/module");
    fs::create_dir_all(&nested).unwrap();

    assert_eq!(
        status_json(&nested, root.path()),
        expected(&repository, json!("main"), &repository)
    );
}

#[test]
fn distinguishes_a_linked_worktree_from_the_main_checkout() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    let worktree = root.path().join("feature");
    init_repository(&repository);
    add_worktree(&repository, &worktree, "feature/one");

    assert_eq!(
        status_json(&worktree, root.path()),
        expected(&repository, json!("feature/one"), &worktree)
    );
    assert_eq!(
        status_json(&repository, root.path()),
        expected(&repository, json!("main"), &repository)
    );
}

#[test]
fn resolves_a_directory_nested_in_a_linked_worktree() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    let worktree = root.path().join("feature");
    init_repository(&repository);
    add_worktree(&repository, &worktree, "feature/one");
    let nested = worktree.join("src");
    fs::create_dir(&nested).unwrap();

    assert_eq!(
        status_json(&nested, root.path()),
        expected(&repository, json!("feature/one"), &worktree)
    );
}

#[test]
fn handles_spaces_in_checkout_paths() {
    let root = TestDir::new();
    let repository = root.path().join("my project");
    let worktree = root.path().join("linked work tree");
    init_repository(&repository);
    add_worktree(&repository, &worktree, "feature/one");
    let nested = repository.join("a directory");
    fs::create_dir(&nested).unwrap();

    assert_eq!(
        status_json(&nested, root.path()),
        expected(&repository, json!("main"), &repository)
    );
    assert_eq!(
        status_json(&worktree, root.path()),
        expected(&repository, json!("feature/one"), &worktree)
    );
}

#[test]
fn reports_a_detached_head_as_no_branch() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);
    git(&repository, &["checkout", "--quiet", "--detach"]);

    assert_eq!(
        status_json(&repository, root.path()),
        expected(&repository, Value::Null, &repository)
    );

    let output = Leme::new(&repository, root.path(), &["status"]).run();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Branch:     (detached HEAD)"), "{text}");
}

#[test]
fn reports_a_branch_without_commits() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "--quiet", "--initial-branch=trunk"]);

    assert_eq!(
        status_json(&repository, root.path()),
        expected(&repository, json!("trunk"), &repository)
    );
}

#[test]
fn treats_a_submodule_as_its_own_main_checkout() {
    let root = TestDir::new();
    let dependency = root.path().join("dependency");
    let repository = root.path().join("project");
    init_repository(&dependency);
    init_repository(&repository);
    git(
        &repository,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "--quiet",
            "add",
            dependency.to_str().unwrap(),
            "vendor/dependency",
        ],
    );
    let submodule = repository.join("vendor/dependency");

    assert_eq!(
        status_json(&submodule, root.path()),
        expected(&submodule, json!("main"), &submodule)
    );
}

#[test]
fn text_and_json_report_the_same_values() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    let worktree = root.path().join("feature");
    init_repository(&repository);
    add_worktree(&repository, &worktree, "feature/one");

    let output = Leme::new(&worktree, root.path(), &["status"]).run();
    assert!(output.status.success());

    let json = status_json(&worktree, root.path());
    let text = format!(
        "Repository: {}\nBranch:     {}\nCheckout:   {}\n\n\
         Integration branch: {}\nPreferred agent:    {}\n",
        json["repository"].as_str().unwrap(),
        json["branch"].as_str().unwrap(),
        json["checkout"].as_str().unwrap(),
        json["preferences"]["integration_branch"].as_str().unwrap(),
        json["preferences"]["preferred_agent"].as_str().unwrap(),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), text);
}

#[test]
fn fails_outside_a_repository() {
    let root = TestDir::new();

    let output = Leme::new(root.path(), root.path(), &["status"]).run();

    let message = failure_message(&output);
    assert!(
        message.contains("is not inside a Git repository"),
        "{message}"
    );
    assert!(message.contains(root.path().to_str().unwrap()), "{message}");
}

#[test]
fn fails_in_a_bare_repository() {
    let root = TestDir::new();
    let repository = root.path().join("project.git");
    fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "--quiet", "--bare"]);

    let output = Leme::new(&repository, root.path(), &["status"]).run();

    let message = failure_message(&output);
    assert!(message.contains("has no checked-out files"), "{message}");
}

#[test]
fn fails_inside_the_git_directory() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);

    let output = Leme::new(&repository.join(".git"), root.path(), &["status"]).run();

    let message = failure_message(&output);
    assert!(message.contains("has no checked-out files"), "{message}");
}

#[test]
fn fails_when_git_is_not_installed() {
    let root = TestDir::new();

    let output = Leme::new(root.path(), root.path(), &["status"])
        .env("PATH", root.path().to_str().unwrap())
        .run();

    let message = failure_message(&output);
    assert!(message.contains("git was not found"), "{message}");
}

#[test]
fn passes_through_git_refusing_an_untrusted_repository() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);

    let output = Leme::new(&repository, root.path(), &["status"])
        .env("GIT_TEST_ASSUME_DIFFERENT_OWNER", "1")
        .run();

    let message = failure_message(&output);
    assert!(message.contains("dubious ownership"), "{message}");
    assert!(message.contains("safe.directory"), "{message}");
}

/// Everything discovery must leave alone in a checkout.
#[derive(Debug, PartialEq, Eq)]
struct RepositoryState {
    head: String,
    symbolic_head: String,
    tree: String,
    status: String,
    index: Vec<u8>,
}

fn repository_state(checkout: &Path) -> RepositoryState {
    let index_path = git(
        checkout,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    );
    let symbolic_head = git_output(checkout, &["symbolic-ref", "HEAD"]);
    RepositoryState {
        head: git(checkout, &["rev-parse", "HEAD"]),
        symbolic_head: String::from_utf8_lossy(&symbolic_head.stdout).into_owned(),
        tree: git(checkout, &["write-tree"]),
        status: git(checkout, &["status", "--porcelain"]),
        index: fs::read(index_path).expect("read index"),
    }
}

/// Leaves staged, unstaged and untracked changes in `checkout`.
fn add_pending_changes(checkout: &Path) {
    fs::write(checkout.join("tracked.txt"), "first\n").unwrap();
    fs::write(checkout.join("staged.txt"), "first\n").unwrap();
    git(checkout, &["add", "tracked.txt", "staged.txt"]);
    commit(checkout, "add files");
    fs::write(checkout.join("staged.txt"), "second\n").unwrap();
    git(checkout, &["add", "staged.txt"]);
    fs::write(checkout.join("tracked.txt"), "second\n").unwrap();
    fs::write(checkout.join("untracked.txt"), "new\n").unwrap();
}

#[test]
fn leaves_the_repository_unchanged() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    let worktree = root.path().join("feature");
    init_repository(&repository);
    add_worktree(&repository, &worktree, "feature/one");
    add_pending_changes(&repository);
    add_pending_changes(&worktree);

    let main_before = repository_state(&repository);
    let linked_before = repository_state(&worktree);
    assert!(!main_before.status.is_empty(), "fixture has no changes");

    for checkout in [&repository, &worktree] {
        for arguments in [&["status"][..], &["status", "--json"][..]] {
            let output = Leme::new(checkout, root.path(), arguments).run();
            assert!(output.status.success());
        }
    }

    assert_eq!(repository_state(&repository), main_before);
    assert_eq!(repository_state(&worktree), linked_before);
}

#[test]
fn leaves_a_detached_head_in_place() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);
    git(&repository, &["checkout", "--quiet", "--detach"]);
    let before = repository_state(&repository);

    let output = Leme::new(&repository, root.path(), &["status"]).run();

    assert!(output.status.success());
    assert_eq!(repository_state(&repository), before);
}
