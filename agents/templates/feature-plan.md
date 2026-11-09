<!--
Feature plan template. Copy this file for a new feature and replace every
<placeholder>. The plan's source of truth is its Notion record; this text is the
shape that record should take. Each step is one proposed commit that the user
reviews, creates and approves. Times use ISO 8601 with an offset.
-->

# <Feature title>

| Field | Value |
| --- | --- |
| Task ID | `<stable task ID, unchanged when branch or worktree change>` |
| Branch | `<feature/branch-name>` |
| Worktree | `<absolute checkout path, or "not created">` |
| Plan version | `<1, 2, …>` |
| Revised at | `<2026-11-09T14:00:00-03:00>` |
| Estimated commits (y) | `<number of steps in this version>` |
| Approved steps (x) | `<number of steps approved with a matching commit>` |

## Goal

<What the user can do when the feature is finished, in one or two sentences.>

## Completion check

<The observable check that shows the whole feature works.>

## Dependencies

- `<task or step ID>`: <what this feature needs from it> — <approved / committed / pending>.

## Steps

Repeat this block for each step, in order. Use the [step template](step.md)
when a step needs more detail than fits here.

### `<step ID>` — <suggested commit purpose>

- **Outcome:** <what is true after this step>.
- **Scope:** <files or areas the step may change>.
- **Depends on:** <earlier step IDs, or "none">.
- **Checks:** <commands or manual checks that verify the outcome>.
- **Stop when:** <the condition at which the agent prepares the review and stops>.

## Revision history

Record every change to the steps or to `y` before work continues. Approved step
IDs and their history are kept across versions.

| Version | Revised at | Change | Reason | y |
| --- | --- | --- | --- | --- |
| 1 | `<time>` | Initial plan | <reason, or "rationale not provided"> | `<y>` |
