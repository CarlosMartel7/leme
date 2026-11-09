# Agent resources: installation map

This folder holds the resources Leme's agents read at work time: role
instructions and planning/reporting templates. The Leme executable does not
install them. A separate installer, written by the user, copies them to the
destination below. This file is that installer's contract: every resource in
`agents/` is listed here with its source and destination.

Executable integration code lives in `src/agents/`, not here.

## Destination

Install the resources into one directory, called `<agents-dir>` below:

- `$XDG_DATA_HOME/leme/agents/` when `XDG_DATA_HOME` is set to an absolute path;
- otherwise `$HOME/.local/share/leme/agents/`.

A relative `XDG_DATA_HOME` is ignored, as it is for Leme's state directory.

The installer:

1. copies each source file to `<agents-dir>/<path relative to agents/>`, keeping
   the folder structure, so relative links between resources keep working;
2. installs exactly the resources listed in the inventory, and nothing else from
   the repository;
3. replaces an older installed copy of a listed file, and leaves other files in
   `<agents-dir>` untouched;
4. does not write into a Git checkout, Claude Code's or Codex's own
   configuration directories, or Leme's state directory.

Agents do not need the resources in their global configuration. Leme supplies
the matching role instructions when it launches each agent: the captain (F09),
the dictator (F10) and feature agents (F07).

## Inventory

| Source in the repository | Installed at | Used by | Purpose |
| --- | --- | --- | --- |
| `agents/instructions/captain.md` | `<agents-dir>/instructions/captain.md` | Captain | Role, permissions and reporting rules |
| `agents/instructions/dictator.md` | `<agents-dir>/instructions/dictator.md` | Dictator | Role, authorized delegation and reporting rules |
| `agents/instructions/feature-agent.md` | `<agents-dir>/instructions/feature-agent.md` | Feature agents | Step execution, review and stopping rules |
| `agents/templates/feature-plan.md` | `<agents-dir>/templates/feature-plan.md` | Captain, dictator | Plan with ordered steps, estimate and revision history |
| `agents/templates/step.md` | `<agents-dir>/templates/step.md` | All roles | One step: outcome, scope, checks, stop condition, progress |
| `agents/templates/decision.md` | `<agents-dir>/templates/decision.md` | All roles | User decision with stated reason and plan impact |
| `agents/templates/review.md` | `<agents-dir>/templates/review.md` | Feature agents | Step review with evidence, checks and commit message |
| `agents/templates/update.json` | `<agents-dir>/templates/update.json` | All roles | Structured update for Leme (fields below) |
| `agents/installation-map.md` | `<agents-dir>/installation-map.md` | Installer, user | This inventory and the update contract |

There are no helper scripts or skills yet. A script will be added, and listed
here, by the feature that defines its inputs and outputs.

## Structured update (`update.json`)

An agent produces this JSON object to give Leme compact display data about a
task. Leme stores it locally; it never needs the agent's Notion or provider
credentials. How the update reaches Leme is defined by the Notion workflow
feature (F12); this section defines its fields.

Times are ISO 8601 strings with a UTC offset. A field marked "or null" is null
when the value is unknown or unavailable. Unknown values are never written as
zero or as an empty string.

| Field | Type | Meaning |
| --- | --- | --- |
| `schema_version` | integer | Version of this format. Currently `1`. |
| `kind` | string | Always `"agent_update"`. |
| `observed_at` | string | When the agent collected the information in this update. |
| `project_id` | string | Leme project ID, as shown by `leme status`. |
| `task_id` | string | Stable task ID. Branch and worktree are attributes of the task, not its identity. |
| `step_id` | string or null | Current step ID; null when no step is active. |
| `plan_version` | integer or null | Version of the plan the step belongs to. |
| `agent.role` | string | `"captain"`, `"dictator"` or `"feature-agent"`. |
| `agent.cli` | string | Agent CLI: `"claude"` or `"codex"`. |
| `agent.session_id` | string or null | Session ID reported by the CLI. |
| `activity` | string | `"working"`, `"awaiting_input"`, `"blocked"`, `"error"` or `"unknown"`. |
| `step_state` | string or null | `"not_started"`, `"in_progress"` or `"awaiting_review"`. Approval is recorded by the user, never reported by an agent. |
| `summary` | string | One sentence the dashboard can show. |
| `highlights` | array of strings | Short lines for the dashboard; may be empty. |
| `checks` | array of objects | Checks run for the current step; may be empty. |
| `checks[].name` | string | Check name, for example `"Tests"`. |
| `checks[].command` | string or null | Command run; null for a manual check. |
| `checks[].outcome` | string | `"passed"`, `"failed"` or `"not_run"`. |
| `checks[].observed_at` | string | When the result was seen. |
| `blockers` | array of strings | What blocks the task; empty when nothing does. |
| `next_action` | string | The next concrete action and who takes it. |
| `notion.status` | string | `"published"`, `"pending"` (saved in `leme-notion-files/`) or `"not_published"`. |
| `notion.record_url` | string or null | URL of the Notion record the update refers to. |
| `notion.publication_id` | string or null | Pending publication ID when `notion.status` is `"pending"`. |
| `notion.source_read_at` | string or null | When the agent last read the task's Notion plan. |
