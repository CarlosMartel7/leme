# Leme

Leme is a Rust command-line project for organizing AI-assisted development in
Linux inside WSL. `leme status` identifies the Git project and checkout, loads
project preferences, and reports locally registered tasks. `leme dashboard`
shows the same project in a full-screen terminal view.

The broader plan includes Herdr workspaces, agent sessions, and live agent data
in the dashboard. Those integrations are not implemented yet.

## Build and install

You need Git on PATH, Rust/Cargo with support for edition 2024, and a C toolchain
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
- The integration branch, preferred agent and clock format from project
  preferences.
- Registered tasks with their IDs, branches, and worktree paths.

A Git worktree is another working folder for the same repository, often on a
different branch. These folders should share a project ID and task list while
reporting their own current branch and checkout path. A submodule is a separate
Git repository nested inside another repository and has its own project identity.

First use creates a project record with an empty task list. There is no task
creation command yet. Status does not change Git history, refs, staged files,
or working files. Outside a checkout it exits with an actionable error.

## Open the dashboard

Run from a Git checkout, in a terminal or a Herdr pane:

```sh
leme dashboard
```

The dashboard shows the project ID, the current branch and checkout, the
project preferences, and the registered tasks, or a message when there are
none. Keys: `?` shows help, `r` refreshes, `↑`/`↓` (or `k`/`j`) select a task,
and `q` or `Esc` quits and returns the terminal to normal. If the project
cannot be loaded, the error is shown on screen and `r` retries.

The local time appears in the top-right corner in the project's
`clock_format`, followed by the time-zone abbreviation, or `--:--` if the
system cannot report it. It is read from the C library, like `date`, and
keeps updating while the project loads or refreshes.

### Focus timer

Once the project has loaded, the line above the status shows a focus/break
timer and the keys that apply:

- `f` starts a 25-minute focus interval and `b` a 5-minute break.
- `p` or `Space` pauses and resumes; `x` cancels the interval.
- When the time runs out the line shows, for example, `Focus done` until you
  start another interval or press `x`. Leme sends no notification.

A running or paused interval must be cancelled before starting a new one. The
countdown uses a clock that system-time changes do not affect, and keeps
counting while the machine is suspended.

The timer is saved in Leme's state database for the project, so every
checkout and every open dashboard of the project share one timer. Closing the
dashboard keeps it: a paused interval reopens paused with the same time left,
a running one reopens with the time that passed deducted, and one that ran out
meanwhile reopens as done. If a timer is already running when another
dashboard starts one, that dashboard shows the existing timer instead.

Refreshing reads Git and Leme's state database only. It does not change the
repository and does not start an agent. The dashboard needs an interactive
terminal and exits with an error when its input or output is redirected.

## Preferences

Create an optional `leme.toml` at the root of the project's primary checkout:

```toml
integration_branch = "main"
preferred_agent = "claude"
clock_format = "24h"
```

These are also the defaults. Supported agent values are `claude` and `codex`;
supported clock formats are `24h` and `12h`.
Every field is optional. Invalid TOML, unknown fields, invalid types, unsupported
agents, and empty branch names produce errors rather than silently using defaults.
The agent preference is reported by status; status does not launch an agent.

Linked worktrees are intended to read the primary checkout's preferences, ignoring
configuration found only in a linked worktree. See [configuration](docs/configuration.md).

## Local state

Leme stores project IDs, task associations and each project's focus timer in
a local SQLite file:

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

The dashboard's end-to-end tests run it in a real pseudo-terminal and are
skipped by `cargo test`. Run them on Linux or WSL with:

```sh
cargo test --locked --test dashboard_e2e -- --ignored --test-threads=1
```

The harness is described in `tests/support/pty.rs`.
