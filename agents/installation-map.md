# Agent resources: installation map

This folder holds the resources Leme's agents read at work time: role
instructions and planning/reporting templates. The Leme executable does not
install them. A separate installer, written by the user, copies them to the
destination below. This file is that installer's contract: every resource in
`agents/` is listed here with its source and destination.

The bundled workflow skills in `agents/skills/` are the exception: Leme's own
skill installer copies them into each agent's skills directory, as described
under [Skills](#skills).

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
| `agents/skills/<name>/` (every file listed in the catalog) | Not in `<agents-dir>`; see [Skills](#skills) | Agents | Bundled workflow skills |
| `agents/skills/catalog.toml` | Not installed | Skill installer | Skill inventory with sources and file hashes |
| `agents/skills/.gitattributes` | Not installed | Git | Keeps skill files byte-for-byte |

There are no helper scripts yet. A script will be added, and listed here, by
the feature that defines its inputs and outputs.

## Skills

`agents/skills/` holds snapshots of the user's existing workflow skills, so a
Leme installation can provide them without the author's home directory. Each
skill is a complete directory: its `SKILL.md` and every supporting file, with
relative references kept intact. The skills own their Notion workflows; Leme
copies their files and never publishes, queries Notion or holds credentials.

`agents/skills/catalog.toml` is the inventory. For each skill it records the
installed name, whether it was requested or is a helper dependency, the source
it was copied from, the bundled skills it invokes or mentions, the tools it
expects at use time, and every file with its size and SHA-256 hash. A file not
in the catalog is not part of the skill.

| Skill | Kind | Copied from | Expects at use time |
| --- | --- | --- | --- |
| `implement-feature-step` | requested | `~/.claude/skills/implement-feature-step` | Git, Notion connector |
| `implement-feature-yolo` | requested | `~/.claude/skills/implement-feature-yolo` | Git, Notion connector |
| `for-dummies` | requested | `~/.codex/skills/for-dummies` | Nothing |
| `plan-parallel-work` | requested | `~/.codex/skills/plan-parallel-work` | Git, Notion connector |
| `create-commit-kanban` | requested | `~/.agents/skills/create-commit-kanban` | Notion connector |
| `grilling` | requested | `~/.agents/skills/grilling` | Nothing |
| `grill-with-docs` (requested as grilling-with-docs) | requested | `~/.agents/skills/grill-with-docs` | Invoking skills by name; `grilling` and `domain-modeling` installed |
| `plan-feature-commits` | requested | `~/.agents/skills/plan-feature-commits` | Git, Notion connector |
| `plan-to-notion` | requested | `~/.agents/skills/plan-to-notion` | Notion connector, Plan collaboration mode |
| `pr-audit` | requested | `~/.agents/skills/pr-audit` | Git, GitHub CLI, Notion connector to publish |
| `domain-modeling` | dependency of `grill-with-docs` | `~/.agents/skills/domain-modeling` | Nothing |

The source paths are provenance only. Installation reads the bundled copies.

Destinations, at the user scope:

- Codex: `~/.agents/skills/<name>/`;
- Claude Code: `~/.claude/skills/<name>/`, or `$CLAUDE_CONFIG_DIR/skills/<name>/`
  when that variable is set.

`leme skills install` is this installer; [workflow skills](../docs/skills.md)
describes its use, results and conflict rules.

The skill installer:

1. installs every skill in the catalog, helper dependencies included, so each
   skill's references resolve;
2. writes each file exactly as bundled, after checking it against the
   catalog's size and hash;
3. leaves other skills in the destination untouched;
4. never runs skill content, and does not change agent permissions,
   authentication or connector settings.

The tools in the last column of the table are prerequisites the user sets up;
installing a skill does not provide them or invoke the skill.

To refresh a snapshot, replace the skill's directory with the new source copy
and update its entry in the catalog, including every file's size and hash.
Do not edit a bundled skill's files in place.

## Structured update (`update.json`)

An agent produces this JSON object to give Leme compact display data about a
task. Leme stores it locally; it never needs the agent's Notion or provider
credentials. No Leme command reads it yet; the feature that does will define
how it reaches Leme. This section defines its fields.

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
| `notion.status` | string | `"published"` or `"not_published"`. |
| `notion.record_url` | string or null | URL of the Notion record the update refers to. |
| `notion.source_read_at` | string or null | When the agent last read the task's Notion plan. |
