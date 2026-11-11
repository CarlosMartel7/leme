# Herdr plugin

Leme ships a Herdr plugin in `plugin/`. Its manifest, `plugin/herdr-plugin.toml`,
declares one action and one pane:

| Item | ID | What it does |
| --- | --- | --- |
| Action | `leme.context` | Opens the context pane next to the pane it was invoked from. |
| Pane | `context` | Runs `leme herdr context` and stays open until Enter is pressed. |

The pane shows the workspace, tab and pane the action was invoked from, as Herdr
reports them. If Herdr is not ready, or does not know the pane, the pane shows
the failed operation and its cause instead. See `leme herdr context --help` and
the README for what the command checks and prints.

## Tested versions

| Component | Version |
| --- | --- |
| Herdr | 0.9.5 (API protocol 22) |
| Plugin manifest | `min_herdr_version = "0.9.5"`, `platforms = ["linux"]` |
| Platform | Linux under WSL 2 |

Other Herdr versions may work. `leme doctor` reports a different API protocol as
uncertain, and the context pane lists it as unconfirmed.

## Install

1. Build and install Leme so `leme` is on `PATH`:

   ```sh
   cargo install --path . --locked
   ```

   Herdr starts plugin commands with the environment of the Herdr server, not of
   the shell you type in. Start Herdr from a shell whose `PATH` includes Cargo's
   executable directory (normally `~/.cargo/bin`). Otherwise the pane shows
   `leme: not found`.

2. Link the plugin from the repository root. The path must be absolute:

   ```sh
   herdr plugin link "$PWD/plugin"
   herdr plugin action list --plugin leme
   ```

   Linking registers the plugin with the running Herdr session and runs nothing.
   Herdr may create the plugin's config and state directories
   (`~/.config/herdr/plugins/config/leme` and `~/.local/state/herdr/plugins/leme`).
   A named session (`herdr --session NAME`) keeps its own plugin list, so link
   the plugin there too.

3. Remove it with `herdr plugin unlink leme`.

## Use

From a shell in any Herdr pane:

```sh
herdr plugin action invoke leme.context
```

The context pane opens to the right of that pane. Press Enter to close it.

To use a key instead, add a binding to Herdr's `config.toml`, as Herdr's plugin
documentation describes, and reload it with `herdr server reload-config`:

```toml
[[keys.command]]
key = "prefix+i"
type = "plugin_action"
command = "leme.context"
description = "show the Leme context"
```

This binding has not been exercised with an attached Herdr client. Choosing
shortcuts that do not conflict with existing bindings is planned work (F05).

## How the invoking pane is passed

Herdr gives an action's process the IDs of the pane it was invoked from
(`HERDR_WORKSPACE_ID`, `HERDR_TAB_ID`, `HERDR_PANE_ID`). A plugin pane, once
open, is a new pane, and its process receives that new pane's ID instead. The
action therefore opens the pane with:

```text
herdr plugin pane open --plugin leme --entrypoint context
    --target-pane <invoking pane> --direction right
    --env LEME_INVOKING_PANE=<invoking pane> --focus
```

The pane runs `leme herdr context --pane "$LEME_INVOKING_PANE"`. Leme then asks
Herdr about that exact pane, so the result does not depend on which pane has
focus when the request runs. Each value is a separate, quoted argument, so IDs
and paths with spaces are passed unchanged.

## Known gaps

- **Overlay and popup placement.** Herdr opens overlay and popup panes over the
  active pane and rejects `--target-pane` for them (`invalid_params`). The pane
  therefore uses `split` placement next to the invoking pane, and stays in the
  layout until Enter is pressed.
- **Opening the pane directly.** Opened with `herdr plugin pane open` instead of
  the action, the pane has no `LEME_INVOKING_PANE` and describes itself (the
  same workspace and tab, but its own pane ID).
- **Checkout.** The workspace's checkout is shown only when Herdr reports one
  for that workspace. Herdr did not report one for a workspace created through
  `herdr workspace create` in a fresh repository during testing.
- **Action contexts.** Herdr's documentation shows only `contexts = ["workspace"]`.
  The manifest uses `["pane"]`, which Herdr 0.9.5 accepted, and the action needs
  `HERDR_PANE_ID`. Herdr's documentation does not list the other values.
- **Plugin context JSON.** `HERDR_PLUGIN_CONTEXT_JSON` names the *focused* pane,
  so Leme does not read it.
- **Leme on `PATH`.** The manifest calls `leme` by name, and does not detect a
  missing or outdated executable.
