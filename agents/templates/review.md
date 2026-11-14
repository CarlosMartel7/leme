<!--
Review template. The feature agent fills it when a step is finished, publishes
it to the task's Notion record as awaiting review, and stops. If publication
fails, give the user the same text and say it was not published. Times use
ISO 8601 with an offset.

Keep three kinds of statement apart:
- Observed: seen directly (command output, files on disk), with source and time.
- Stated: said by the user or another agent, attributed to them.
- Inferred: your own conclusion, with the evidence it rests on.
Never report a check as passed unless you ran it and saw it pass.
-->

# Review: `<step ID>` — <suggested commit purpose>

| Field | Value |
| --- | --- |
| Task ID | `<task ID>` |
| Step ID | `<step ID>` |
| Plan version | `<plan version>` |
| Branch / worktree | `<branch>` at `<absolute worktree path>` |
| Base commit | `<short SHA of HEAD the changes were made on>` |
| Prepared at | `<time>` |
| Publication | `<published: Notion URL / not published: reason>` |
| State | awaiting review |

## Summary

<One or two sentences: what the step changes and why.>

## Changes

Group by purpose, not by file. Mark each file as added, modified or deleted.

- `<file>` (<added / modified / deleted>): <what changed and what it enables>.

## Checks

| Check | Command | Result | Observed at |
| --- | --- | --- | --- |
| <name> | `<command>` | <passed / failed: summary / not run: reason> | `<time>` |

## Evidence

### Observed

- <fact> — source: <command or file>, at `<time>`.

### Stated

- <statement> — said by <the user / agent>, at `<time>`; recorded in <decision link, or "this review">.

### Inferred

- <conclusion> — based on <evidence>; <not verified / verified by …>.

## Open questions and limits

- <question, limitation or work deferred to another step — or "none">.

## Blockers

- <blocker — or "none">.

## Suggested commit message

```text
<type>(<area>): <subject>

<optional body explaining why>
```

## How to inspect

```sh
git status
git diff
git diff -- <new or specific file>
```

## Next action

The user reviews the changes, creates the commit and approves this step. The
agent does not start `<next step ID>` until then. Any edit to the files above
after `<prepared at>` makes this review stale.

---

## Example

### Review: `T-12-S2` — feat(state): record agent sessions

| Field | Value |
| --- | --- |
| Task ID | `T-12` |
| Step ID | `T-12-S2` |
| Plan version | `3` |
| Branch / worktree | `feature/feature-agents` at `/home/user/worktrees/leme/feature-agents` |
| Base commit | `4f2c9a1` |
| Prepared at | `2026-11-09T14:41:00-03:00` |
| Publication | published: `https://www.notion.so/<review-record>` |
| State | awaiting review |

#### Summary

Adds a `sessions` table so Leme can remember which agent conversation belongs
to each task. The session ID may be empty until the agent reports it.

#### Changes

- `src/state/migrations/002_sessions.sql` (added): creates the `sessions` table
  with a nullable `session_id`, keyed by task.
- `src/state/mod.rs` (modified): runs the new migration after `001_project.sql`.

#### Checks

| Check | Command | Result | Observed at |
| --- | --- | --- | --- |
| Formatting | `cargo fmt --all -- --check` | passed | `2026-11-09T14:38:10-03:00` |
| Tests | `cargo test --locked` | passed, 47 tests | `2026-11-09T14:39:02-03:00` |
| Lint | `cargo clippy --locked --all-targets -- -D warnings` | passed | `2026-11-09T14:39:40-03:00` |

#### Evidence

##### Observed

- `git status` lists `src/state/migrations/002_sessions.sql` as untracked and
  `src/state/mod.rs` as modified, at `2026-11-09T14:40:30-03:00`. The index is
  empty.
- `HEAD` is `4f2c9a1`, the commit the user approved for `T-12-S1`.

##### Stated

- The user chose to allow an empty session ID: "Codex only shows the session ID
  after the first turn, and I want the launch recorded even if it fails before
  that." Recorded in decision "allow a missing session ID",
  `2026-11-09T14:20:00-03:00`.

##### Inferred

- Rows created before the agent reports its ID will need the ID filled in
  later. This follows from the nullable column; it is not tested in this step
  and is planned as `T-12-S5`.

#### Open questions and limits

- No test covers two launches of the same task racing to insert a row; the
  plan assigns that to `T-12-S3`.

#### Blockers

- None.

#### Suggested commit message

```text
feat(state): record agent sessions

Keep the session ID optional because some agent CLIs report it only after
the first turn.
```

#### How to inspect

```sh
git status
git diff
git diff --no-index /dev/null src/state/migrations/002_sessions.sql
```

#### Next action

The user reviews the changes, creates the commit and approves `T-12-S2`. The
agent does not start `T-12-S3` until then.
