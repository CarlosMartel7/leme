# Feature workspaces

`leme feature open TASK` gives a task its own Git checkout and Herdr
workspace, with a default set of tabs. Run it from any checkout of the
project.

```sh
leme feature open T-12 --branch feature/search    # first time
leme feature open T-12                            # afterwards
leme feature open T-12 --branch feature/search --path ../search --yes
leme feature open T-12 --json
```

| Option | Meaning |
| --- | --- |
| `--branch NAME` | The task's feature branch. Required the first time; afterwards it may only repeat the recorded branch. |
| `--path PATH` | Where to create the checkout when the branch has none. Relative paths start at the current directory. |
| `--yes` | Create the branch and worktree without asking. |
| `--json` | Print the result as JSON, with the same values as the text. |
| `--timeout SECONDS` | How long each Herdr request may take (default 30). |

## What it resolves first

Before changing anything, Leme works out:

1. **The project:** the repository containing the current directory, through
   its main checkout.
2. **The task and branch:** the task ID is yours to choose, such as a ticket or
   card key. A task owns one branch and a branch belongs to one task.
3. **The base:** `integration_branch` from `leme.toml` (default `main`). A new
   branch always starts from this branch, never from whatever happens to be
   checked out where the command runs, including the main checkout.
4. **The checkout:**
   - If a linked worktree already has the branch, that worktree is used.
   - A branch checked out in the main checkout is refused: that checkout is
     shared by the project. A worktree whose directory is missing is refused
     with a hint to run `git worktree prune`.
   - Otherwise a new worktree is planned at `--path`, at the path recorded by an
     earlier attempt, or at `~/.herdr/worktrees/<repository directory>/<branch
     with / as ->`, the location Herdr itself uses. The path must not exist.
     An existing branch is checked out there; a new branch starts at the base,
     which must exist.

Herdr is then checked the way `leme doctor` checks it. Nothing has changed at
this point.

## Authorization

Creating a branch or worktree changes Git, so Leme shows what it would do and
asks:

```text
Opening task T-12 changes Git:
  - create branch feature/search from main, the integration branch
  - in a new worktree at /home/me/.herdr/worktrees/project/feature-search
Herdr then opens it in a new workspace with agent, neovim and tests tabs.
Create them? [y/N]
```

Only `y` or `yes` continues. Without a terminal to ask in, the command stops
with the same description and changes nothing unless `--yes` was given.
Opening an existing checkout changes no Git state and does not ask.

## What it changes

1. The task is recorded in Leme's state database with its branch and checkout.
2. Herdr creates the worktree (`herdr worktree create`, which runs
   `git worktree add`) or opens the existing one (`herdr worktree open`), and
   focuses its workspace. Herdr keeps one workspace per checkout, so a second
   invocation reuses it; a workspace that was closed is reopened and its new ID
   recorded. Workspace names you chose are never changed.
3. The default tabs are opened in that workspace, each with the checkout as an
   explicit working directory:

   | Tab | Contents |
   | --- | --- |
   | `agent` | A shell left ready for the agent launcher. It gets focus. |
   | `neovim` | Neovim (`nvim`), started in the checkout. |
   | `tests` | A shell for running tests. |

   The initial shell tab Herdr creates with a new workspace is closed once all
   three exist. When Neovim is not on `PATH`, the neovim tab keeps a shell and
   the report says so.

Every tab's shell gets these variables:

| Variable | Value |
| --- | --- |
| `LEME_PROJECT_ID` | The project's ID in Leme's state database. |
| `LEME_TASK_ID` | The task ID. |
| `LEME_BRANCH` | The task's branch. |
| `LEME_CHECKOUT` | The checkout's path. |

## Interrupted setup

Each tab is recorded as soon as Herdr creates it, and marked ready once its
command has been started. If a tab cannot be opened, the command stops, names
the tab and Herdr's reason, and keeps the checkout and the tabs already opened.
Running `leme feature open TASK` again:

- keeps recorded tabs that Herdr still lists in the task's workspace;
- finishes a tab whose command had not been started yet;
- opens only the tabs that are missing, including any you closed;
- never touches the checkout's files, so local changes survive.

Tabs recorded for a workspace that has since been replaced are opened again in
the new workspace.

## Known gaps

- **Resource collisions.** Leme has no record of ports, containers or test
  databases yet, so it cannot report conflicts between features. Projects that
  share such resources still need to separate them by hand, for example with
  `LEME_TASK_ID`.
- **Neovim detection.** Leme looks for `nvim` on its own `PATH`. The tab's shell
  is started by the Herdr server, whose `PATH` may differ.
- **Default location.** The default checkout path follows Herdr 0.9.5's own
  convention; pass `--path` to choose another place.
- **Source workspace.** On its first worktree request in a repository, Herdr may
  also open a workspace for the main checkout.
