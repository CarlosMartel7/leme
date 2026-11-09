<!--
Single step template. One step is one proposed commit. The feature agent fills
the header before starting, keeps "Progress" current while working, and stops
at "Stop when". Times use ISO 8601 with an offset.
-->

# `<step ID>` — <suggested commit purpose>

| Field | Value |
| --- | --- |
| Task ID | `<task ID>` |
| Step ID | `<step ID>` |
| Plan version | `<plan version this step belongs to>` |
| Branch / worktree | `<branch>` at `<absolute worktree path>` |
| State | `<not started / in progress / awaiting review / approved>` |
| Previous step | `<step ID> — approved with commit <SHA>`, or "first step" |
| Observed at | `<time this header was last checked>` |

## Outcome

<What is true after this step, stated so it can be checked.>

## Scope

- **May change:** <files or areas>.
- **Must not change:** <files or areas that belong to other steps or features>.

## Checks

| Check | Command or procedure | Required |
| --- | --- | --- |
| <name> | `<command>` | <yes / no> |

## Stop when

<The condition at which the agent prepares the review and stops for the user.>

## Progress

- **Blockers:** <what blocks the step, what was tried, what is needed — or "none">.
- **Next action:** <the next concrete action, and who takes it>.
- **Scope notes:** <work found that belongs to another step, not done here>.
