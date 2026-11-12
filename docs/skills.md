# Workflow skills

Leme ships copies of the workflow skills its planning and review process uses.
A skill is a folder with a `SKILL.md` file of instructions that an AI coding
agent (Codex or Claude Code) can follow on request. `leme skills install` copies
these folders into an agent's user skills directory, where the agent finds them
in every project.

## Install

```sh
leme skills install --target codex --dry-run
leme skills install --target codex
leme skills install --target claude
leme skills install --target both --json
```

`--target` is required. `--dry-run` reports what would happen and writes
nothing. `--json` prints the same report as JSON.

| Target | Skills directory |
| --- | --- |
| `codex` | `$HOME/.agents/skills/<name>` |
| `claude` | `~/.claude/skills/<name>`, or `$CLAUDE_CONFIG_DIR/skills/<name>` when `CLAUDE_CONFIG_DIR` is set |
| `both` | Both of the above |

These are the user-scope locations documented by Codex and Claude Code.
`CLAUDE_CONFIG_DIR` must be an absolute path. Leme reads it from the
environment only; a value set inside Claude Code's own settings file is not
visible to Leme, so check the directory shown in the report. Installed skills
appear in agent sessions started afterwards.

The skills are embedded in the `leme` executable when it is built, so
installing needs neither network access nor a Leme checkout.

## Bundled skills

All bundled skills are installed together. `domain-modeling` is included as a
helper because `grill-with-docs` invokes it.

| Skill | Expects at use time |
| --- | --- |
| `implement-feature-step` | Git, Notion connector |
| `implement-feature-yolo` | Git, Notion connector |
| `for-dummies` | Nothing |
| `plan-parallel-work` | Git, Notion connector |
| `create-commit-kanban` | Notion connector |
| `grilling` | Nothing |
| `grill-with-docs` | An agent that can invoke skills by name |
| `plan-feature-commits` | Git, Notion connector |
| `plan-to-notion` | Notion connector, Plan collaboration mode |
| `pr-audit` | Git, GitHub CLI (`gh`), Notion connector to publish audits |
| `domain-modeling` | Nothing |

Leme does not install or configure these tools. The skills keep their own
Notion workflows; Leme does not publish to Notion or hold credentials.
`agents/skills/catalog.toml` lists each skill's source and every file with its
SHA-256 hash, and [the installation map](../agents/installation-map.md)
describes the bundle.

## What the installer does

Before writing anything, Leme checks the bundled files against the catalog's
sizes and hashes, then inspects every selected destination. Each skill gets
one of these results:

| Result | Meaning |
| --- | --- |
| `would install` / `installed` | The destination did not exist. |
| `would update` / `updated` | It held an older Leme copy that nobody changed. |
| `unchanged` | It already matches the bundled skill. Nothing is written. |
| `conflict` | It exists and is not Leme's to replace (see below). |
| `failed` / `not attempted` | Writing failed, which stops that target. |

A conflict is a directory Leme did not install that differs from the bundled
skill, a Leme copy changed since it was installed, a file or symbolic link
where the skill directory belongs, or a skill directory containing a symbolic
link. Any conflict stops the whole run before anything is written, for every
target, and the command exits with status 1. Move the directory aside (or keep
your edits under another skill name) and run again. A directory that already
matches the bundled skill exactly is accepted as installed.

Each skill directory is replaced as a whole: the new copy is written next to
it and renamed into place, so an agent never sees a half-written skill.
Installed files are regular files with mode 644. Other skills and files in the
skills directory are never touched.

Leme records what it installed in `.leme-skills.toml` in each skills directory,
with the hash of every installed file. This record is how an unchanged older
copy is told apart from local edits. `.leme-skills.lock` keeps two Leme runs
from writing the same directory at once.

With `--target both`, a failure in one target (for example a read-only
directory) does not undo the other. The report lists what each target
completed, and the command exits with status 1.

Installing never runs skill content and does not change agent settings,
permissions, authentication, connectors or MCP servers. Installing
`implement-feature-yolo` does not run it or authorize commits.
