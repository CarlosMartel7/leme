# Leme

Leme is a Rust command-line project for organizing AI-assisted development in
Linux inside WSL. It currently has two commands: `leme status` identifies the
Git project and checkout, loads project preferences, and reports locally
registered tasks; `leme doctor` checks which Leme actions this installation
supports.

The broader plan includes Herdr workspaces, agent sessions, and a terminal
dashboard. Those integrations are not implemented yet; `leme doctor` only checks
whether the tools they will need are ready.

## Build and install

You need Git on PATH, Rust/Cargo 1.89 or later, and a C toolchain
for the bundled SQLite dependency. A separate SQLite server is not required.

From the repository root:

```sh
cargo build --locked
cargo install --path . --locked
```

Ensure Cargo's executable directory (normally `~/.cargo/bin`) is on PATH.

## Check a project

Run from a Git checkout or any directory inside it:

```sh
leme status
leme status --json
leme status --help
```

Without installing, run `cargo run --locked -- status` from the Leme repository.
To inspect another repository with the built executable, run it there using its
absolute path.

Status reports:

- A locally persisted project ID and the primary checkout path.
- The current checkout path and branch; detached HEAD has no branch.
- The integration branch and preferred agent from project preferences.
- Registered tasks with their IDs, branches, and worktree paths.

A Git worktree is another working folder for the same repository, often on a
different branch. These folders should share a project ID and task list while
reporting their own current branch and checkout path. A submodule is a separate
Git repository nested inside another repository and has its own project identity.

First use creates a project record with an empty task list. There is no task
creation command yet. Status does not change Git history, refs, staged files,
or working files. Outside a checkout it exits with an actionable error.

## Check the environment

Run from a project checkout to see which Leme actions will work here:

```sh
leme doctor
leme doctor --json
leme doctor --timeout 10
```

Doctor looks for Git (2.31 or later), Herdr and its running server, Neovim, the
Claude Code and Codex CLIs, `gh`, and `git-flow`. It also checks the project
repository, its `leme.toml`, and the state directory. Each action is reported
as:

- `available`: every check it needs passed.
- `uncertain`: nothing failed, but compatibility could not be confirmed, for
  example a Herdr whose API protocol differs from the one Leme was tested with.
- `unavailable`: a check it needs failed. Each failed check names the manual
  correction, and other actions are unaffected; a missing `gh`, for example,
  disables only GitHub queries.

Herdr is checked with `herdr status server --json` and `herdr api schema --json`,
which list the server state and the operations it supports. Rust, Cargo and a C
compiler are listed separately, because they are needed only to build Leme.

Doctor only reads. It runs version and status commands, never authentication
commands, and does not install or configure tools or change repository files.
The only write is an empty probe file, created and removed at once, that shows
whether the state directory accepts new files. An existing database is opened
for writing to confirm `leme status` can update it, but nothing is written, and
the database is never created or migrated. Each tool has `--timeout` seconds (default 5) to answer.
The exit status is 0 whenever the report is printed, since missing optional
tools are part of the diagnosis.

## Preferences

Create an optional `leme.toml` at the root of the project's primary checkout:

```toml
integration_branch = "main"
preferred_agent = "claude"
```

These are also the defaults. Supported agent values are `claude` and `codex`.
Every field is optional. Invalid TOML, unknown fields, invalid types, unsupported
agents, and empty branch names produce errors rather than silently using defaults.
The agent preference is reported by status; status does not launch an agent.

Linked worktrees are intended to read the primary checkout's preferences, ignoring
configuration found only in a linked worktree. See [configuration](docs/configuration.md).

## Local state

Leme stores project IDs and task associations in a local SQLite file:

- `$XDG_STATE_HOME/leme/leme.db` when XDG_STATE_HOME is absolute.
- Otherwise, `$HOME/.local/state/leme/leme.db`.

The database is normally outside Git checkouts, so removing a feature worktree
does not remove its project record. Keep the state directory outside disposable
worktrees when setting XDG_STATE_HOME yourself. The database is local to this
state directory; IDs are not a cross-machine identity or a Git commit hash.

Schema changes run through transactional migrations. A database written by a
newer, unsupported schema version is rejected with an update message. Planning
and delivery reports belong in Notion rather than this database.

## Known limitation

The audited status implementation misidentifies the primary checkout when run
from a linked worktree of an absorbed submodule (one whose Git metadata lives
under its parent's `.git/modules`). It can create a second project record and
read preferences from the metadata directory. Existing task records are not
deleted, but may be absent from the task list shown under the second ID. Ordinary
worktrees and primary submodule checkouts are covered by the current tests;
the combined layout needs a correction and regression coverage.

## Development

PRs target `main`; there is no designated development integration branch.
See [AGENTS.md](AGENTS.md) for contribution and agent instructions.

Run checks from the repository root:

```sh
cargo fmt --all -- --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

Tests use disposable Git repositories and isolated SQLite state. Add `--offline`
to Cargo test/Clippy commands when dependencies are already cached.
