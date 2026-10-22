# Develop against an installed Lucent session

The opt-in development command rebuilds Lucent after source edits and restarts
the installed desktop with the new ordinary clients. Run the watcher alongside
your existing Lucent session. It does not install or activate Lucent, register an
autostart watcher, or transfer a build to another machine.

This workflow replaces a running process. Hyprland and your application windows
keep running, while Lucent starts a fresh component tree. Open panels, launcher
queries and other transient UI state reset. Saved preferences reload normally.

## Choose the shortest path for your change

| Change | How to apply it |
| --- | --- |
| Launcher size, widget layout, wallpaper, palette or another supported preference | Use the native UI or `lucent-cli`; these changes already apply live. |
| Rust components, framework implementation or bundled client artwork | Build the three ordinary clients and apply the resulting bundle. |
| `design/tokens.json` | Regenerate the shared Rust/CSS/config outputs, then rebuild and apply the ordinary clients. |
| Secure client code, authentication/session integration, supervisor, user units or Hyprland rules | Use the relevant reviewed manual installation workflow. These changes are outside the developer reload. |
| Astro documentation | Use the separate website development/build workflow; a desktop rebuild does not update the site. |

Edit the design-token source rather than generated files. A token change can
produce compositor or application integration outputs as well as Rust constants.
The reload bundle includes seven theme support files: `palettes.json` and the
dark/light versions of `colors.toml`, `gtk.css` and `hyprlock.conf`, sourced from
`configs/lucent/` and installed under `~/.local/lib/lucent`. They keep runtime
theme exports on the same design source as the binaries. Generated Hyprland
integration rules still use their normal manual installation workflow.

The current palette is a saved set of colors. Editing a named palette in the
token catalog preserves that saved selection; reselect the palette in the UI
or with its normal command, such as `lucent-cli palette ocean`, to adopt the
updated catalog colors. Layout and component-token changes still rebuild normally.

## Start explicitly

Run as the normal desktop user, from the repository root on the machine running
Lucent. In the lab, that means the guest checkout; the watcher does not copy host
edits across SSH.

```sh
python3 scripts/lucent-dev.py doctor
python3 scripts/lucent-dev.py once
python3 scripts/lucent-dev.py watch
```

`doctor` checks whether the existing installation and session can support a
reload. `once` builds and applies one source revision. `watch` remains attached
to your terminal, starts with an initial build and repeats that workflow after
relevant edits. Running the tool without a subcommand also selects `watch`.
Starting the watcher is an explicit development action; normal logins do not
start it.

The watcher can build while the screen is locked. If the session is unsafe for
apply, it retains the successful pending bundle and waits for the guard to clear.
Unlock, close authentication prompts and display-preview confirmations, and let
preference saves finish for the apply to proceed. Newer edits supersede pending
source before apply begins. Freshness is checked when the bundle is published
and immediately before apply; edits arriving after that boundary queue the next
cycle. Keep other builders and installers out of this checkout while the
development tool owns its lease.

For a separate build/review/apply sequence:

```sh
python3 scripts/lucent-dev.py build
python3 scripts/lucent-dev.py apply /path/to/the/printed/bundle
```

Use the bundle path printed by `build`. An apply validates that bundle before
changing the installation. A successful compile by itself does not change the
running desktop. `build` can run separately on a compatible build machine; copy
the printed bundle to the target explicitly and run `apply` there. The receiver
needs the developer tool and existing installation, but `apply` and `doctor` do
not invoke Cargo or Git.

## Watch scope and build controls

The watcher observes `lucent/`, `design/`, `configs/lucent/` and the design-token
and app-icon generator scripts through Linux file notifications. It excludes
documentation, the Astro site, reports, caches, targets and visual fixtures.
Editing the developer tool itself requires restarting that tool.

Release is the default profile. For a debug build while iterating:

```sh
python3 scripts/lucent-dev.py watch --profile debug
```

Common controls include `--root PATH`, `--target-dir PATH`, `--debounce SECONDS`
and `--ready-timeout SECONDS`. Options can appear before or after the subcommand.
The debounce default is 0.35 seconds. The target defaults to `lucent/target` in
the selected checkout, with `CARGO_TARGET_DIR` respected when set. Bundles are
kept in the user's `lucent-dev/bundles` cache; use the printed path rather than
guessing a bundle name.

Each build uses one Cargo job inside a separate user unit with a one-core CPU
quota, a 900 MiB memory high threshold, a 1400 MiB memory maximum, 256 tasks and
a 30-minute runtime limit. The memory high threshold applies pressure; it is not
a second hard cap. An exhausted build fails without replacing the desktop.

Readiness defaults to a 45-second timeout and requires at least 35 seconds of
startup survival. The new desktop can already be visible during that check.
Edits during an apply are handled after the current bounded transaction ends;
they do not interrupt a half-installed set of clients.

## What an edit triggers

The watcher coalesces edits and builds the current source with one Cargo job.
Design-token generation precedes compilation when needed. The release bundle
contains `lucent-desktop`, `lucent-cli` and `lucent-menu`, the seven theme support
files, and metadata used to validate their contents.

The build runs with bounded resources. A Rust or token-generation error leaves
the installed desktop running and prints the failure for you to fix. Edits
that supersede a build must be processed before that result can become the
next installed revision. Changing the source does not submit GPU frames to the
existing process: the new implementation begins running after a successful
apply and restart.

The apply path:

1. Validates the bundle and the existing owned Lucent service/session.
2. Saves the original managed files and a durable transaction journal under
   `~/.local/state/lucent/dev-reload` and stages their replacements.
3. Stops the verified Lucent service, replaces the three ordinary executables
   and their theme support files, and starts only `lucent.service` again.
4. Checks the new process identity and shared native surface readiness, then
   observes survival beyond the supervisor's startup deadline.
5. On failure, restores files that are still the transaction's own postimages
   and verifies the restored desktop. Detected independent file changes stop
   automated recovery; retained journals and backups explain what needs review.

Each managed file replacement is atomic. The complete update is coordinated
while the desktop service is stopped; it is not one filesystem-wide atomic
operation. Checked exchanges validate the displaced file; ambiguous ownership
retains the entries for manual recovery. This is not an adversarial filesystem
compare-and-swap guarantee. Saved preferences, normal service configuration,
application windows and compositor configuration are outside this transaction.

The tool does not rebuild/install `lucent-lock`, `lucent-greeter` or
`lucent-polkit`, restart the login manager, modify PAM, or refresh the session
supervisor and integration helpers. Framework changes that require those
consumers to move together need the normal coherent installation procedure.

## Stop and recover

Press **Ctrl+C** in the watcher terminal to stop watching and building. The
normal installed desktop keeps running. If interruption arrives during an
apply, let its guarded recovery finish and inspect the reported result.

Useful checks from your normal session:

```sh
systemctl --user status lucent.service
lucent-cli inspect
journalctl --user -u lucent.service -n 60
```

Keep a terminal or existing SSH route available while developing. If automatic
recovery reports a foreign file change or an incomplete transaction, retain its
journal and backups. Use the reported identities to review the affected files
before manual recovery; starting another installer over them can obscure the
original failure. A forced kill or power interruption can leave a journal needing
manual recovery; the tool does not install a startup recovery daemon. Stock-shell
rollback is a separate explicit operation in
[the installation guide](install-and-rollback.md).

## Source and validation boundary

The entry point is `scripts/lucent-dev.py`. File observation, token generation
and builds belong to the developer tool; installation and recovery belong to
its deployment adapter. The desktop's domain, ports, components and rendering
loop do not depend on this development tool.

The source workflow and its tests are prepared in this change. Native watcher,
apply, readiness and rollback acceptance still require an actual installed
session run. A prepared command, mocked transaction or successful build is not
that native result. Record the source and bundle hashes when reporting one.
