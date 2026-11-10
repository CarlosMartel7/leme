# Leme: project instructions

## Project and stack

Leme organizes AI-assisted development for one repository at a time. The target
is Linux inside WSL. The implemented CLI exposes `leme status` and a first
`leme dashboard` screen; Herdr integration and live agent data are planned work,
not existing features.

This is a Rust 2024 Cargo project. Clap handles CLI parsing, Serde handles JSON,
TOML holds project preferences, and rusqlite manages a local SQLite database.
SQLite is bundled. Ratatui and Crossterm draw the dashboard.
Do not introduce Python or Node as requirements for the core executable.

## Branches and human review

PRs target `main`. There is no designated `develop`/development integration
branch. A local branch named `develop` does not change this policy. Verify the
actual PR target and current remote state rather than inferring policy from
branch names. Do not create an integration branch without user direction.

The user reviews and creates commits. Do not stage, commit, push, merge, or
rewrite contributor history without explicit authorization. Preserve unrelated
work. Report proposed corrections before applying PR-audit adjustments.

## Ownership and invariants

- `src/commands/`: CLI dispatch and output; text and JSON use the same values.
- `src/git/`: live branch, current checkout, and shared repository discovery.
  Discovery must leave refs, history, index, and working files unchanged.
- `src/dashboard/`: snapshot collection, screen state (`app`) and drawing
  (`view`), kept separate so state can be tested without a terminal.
- `src/config.rs`: `leme.toml` from the primary checkout; linked worktrees share
  preferences. Missing settings use defaults; invalid settings report errors.
- `src/state/`: durable project identity, task associations, and schema migrations.
  Keep operational state outside disposable worktrees. Use transactions and
  database constraints for migrations and concurrent registration.
- Git owns current branch/checkout facts; SQLite owns operational records;
  Notion owns plans, decisions, and delivery/audit reports.

One repository's checkouts must resolve to one project record and task list.
A submodule is a separate repository, but its linked worktrees must still share
its identity and preferences. Test combinations of supported Git layouts, not
only each layout separately. Fix shared discovery errors at their owning
boundary, then verify configuration and persistence consumers too.

The default state file is `$XDG_STATE_HOME/leme/leme.db`, falling back to
`$HOME/.local/state/leme/leme.db`; relative XDG_STATE_HOME is ignored. Tests
must use disposable repositories and isolated state, never the user's database.

## Verification

Run from the repository root, with the committed Cargo.lock:

```sh
cargo fmt --all -- --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

Use `--offline` when dependencies are cached and network is unnecessary. For
untrusted PRs, inspect all changes before executing builds/tests and use an
isolated environment without credentials. Record exact audited commits and
which checks ran; do not describe absent hosted checks as passing. Recheck
current workflows rather than assuming the first audit's CI state persists.

Behavioral fixes need a regression test that fails on the behavior without the
fix and passes with it. Include migration rollback, project isolation, and
concurrent access where relevant. Review path handling for the supported Linux/
WSL environment; do not claim native Windows support from a Linux test run.

## Documentation and reports

Keep README usage and docs/configuration.md aligned with implemented behavior.
Separate planned capabilities from working commands. Explain audit findings in
plain language: purpose, triggering case, expected/actual behavior, user impact,
evidence, and proposed fix. Assume the user is unfamiliar with Rust and libraries.
Use `for-dummies` when a deeper beginner explanation is requested.

Publish PR audits under the top-level `PR Audits` page in the Leme Notion
teamspace, with a child page named after the PR. Do not publish them privately
or under Project Summary. Existing destination:
https://app.notion.com/p/4114624efa8881568f5cdd2739229e3a

Keep edits to existing skills minimal and preserve unrelated wording and
structure. Only perform major skill rewrites when the user asks or explicitly
approves them.
