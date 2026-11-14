# Project configuration

Leme reads a project's preferences from `leme.toml` in the root of its main
checkout. All settings are optional.

```toml
# Branch that feature branches start from and merge back into.
integration_branch = "main"

# Agent started for a feature: "claude" or "codex".
preferred_agent = "claude"

# How the dashboard shows the local time: "24h" (14:05) or "12h" (2:05 PM).
clock_format = "24h"
```

## Lookup

Leme looks in one place: the main checkout of the repository containing the
current directory. A linked worktree therefore uses the same preferences as
every other checkout, and a `leme.toml` that exists only in a linked worktree
is ignored. A submodule is a separate repository with its own main checkout.

## Defaults

| Setting | Default |
| --- | --- |
| `integration_branch` | `main` |
| `preferred_agent` | `claude` |
| `clock_format` | `24h` |

The defaults apply to any setting the file leaves out, and to all settings
when the file does not exist.

## Errors

Leme stops and names the file when it cannot use it, instead of falling back
to the defaults. An error quotes the offending line and field for invalid
TOML, an unknown setting, a value of the wrong type, an unsupported agent or
clock format, or an empty branch name.
