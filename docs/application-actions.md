# Application desktop actions

Secondary-click an application result, or select it with the launcher arrows and
press the context-menu key or Shift+F10, to open its declared desktop actions.
The chooser stays on the launcher surface. Up/Down and the wheel move through
the choices; Enter dispatches the selected action. Back or the first Escape
returns to the same application query and selection. A second Escape closes the
launcher. An application without declared actions shows an empty state.

The chooser uses the existing launcher geometry and design-system button,
status, color and spacing recipes. It renders a bounded visible row range so
the last action remains reachable on a narrow or short output. Action-specific
icons are retained as catalog metadata; this chooser displays the prepared
application icon and action labels.

## Captured authority and completion

`ApplicationActionId` is a checked, case-sensitive identifier: 1–128 ASCII bytes
from letters, digits and hyphens. `Application.actions` contains ordered
`ApplicationAction` values with an identifier, label, icon and typed launch
description. The pure `launch_application_action` use case requires exactly one
valid declared action before calling `ApplicationPort::launch_action`.

The launcher captures the complete application descriptor when it opens the
chooser. A chosen action carries that unchanged descriptor and exact action ID
to the composition root. The child validates against the current catalog again
before emitting its intent; the root checks the current exact descriptor before
scheduling work. The production XDG adapter freshly discovers the application
and requires full descriptor equality and a unique action before native
dispatch. A changed same-key command, label, executable directory, terminal
preference or action list represented in that descriptor cannot silently retarget
an old menu. Path/Terminal affect descriptors with executable commands or
fallbacks; changing them on a native-only entry with no Exec leaves its dispatch
unchanged. Query/mode changes, catalog
replacement/removal, unavailable observation and closing retire the chooser.

Each action attempt and menu has its own checked revision. Only one action
attempt is retained. Closing or going back does not revoke an admitted native
call, admit a duplicate, or replay it. Completion clears only that exact attempt
and changes only its originating menu. A successful dispatch acknowledgement
closes that menu; a failure remains local there. A late result cannot close or
write an error into a newer chooser. Queue rejection/panic uses a task-local
completion; an ambiguous worker failure is labelled uncertain. Unrelated root
command success does not erase an action failure.

Actions always dispatch their chosen action rather than using the main launch
or prefer-running-window focus policy. Dispatch acknowledgement does not prove
that a window opened. Native application activation remains a synchronous call
with a five-second deadline on the existing controls queue. A stalled application
can therefore delay other controls on that queue. This milestone adds no worker
or general scheduling facility.

## Native adapter policy

Production parsing retains at most 32 valid listed actions per application,
preserving listed order and the first occurrence of each ID. Missing required
fields, malformed/duplicate action groups and oversized action descriptions are
skipped. Selected localized main/action names must be nonempty, contain no
controls and occupy at most 1,024 bytes. Main comments are bounded to 4,096
bytes. Parsed/expanded commands retain at most 4,096 argument words including
the program and icon expansion, and at most 1 MiB of expanded text. Commands
are rejected as a whole rather than truncated. An unusable main entry is
omitted. These are implementation resource policies, not desktop-entry format
limits.

The existing 1 MiB source-file and 16 MiB retained-catalog budgets also apply;
the catalog accounting includes action/string/argument storage and retained
allocations, including empty argument entries. These limits do not bound total
process, decoded image or native transport memory.

Each action uses its own localized Name/Icon/Exec and the main entry's Path and
Terminal settings. D-Bus-activatable entries use the selected action's desktop
service activation and only that action's eligible command fallback. No action
uses the main Exec as a fallback. Denied or uncertain D-Bus calls do not trigger
a command fallback. Native action parameters/platform data are empty, and
application units explicitly remove ambient activation/startup token variables.

## Literal IPC and inspection

```text
lucent-cli launcher actions org.example.App.desktop
lucent-cli launcher action New-Window
```

The first command captures the current catalog descriptor; an active application
query must still contain that result. The second requires a currently open
captured chooser and its unique action. It cannot dispatch an arbitrary app/action
pair after the chooser retires. Standard `launcher open` keeps its existing
behavior of starting a fresh query.

`inspect` exposes `client.application_actions` with open/application/selected,
ordered action IDs and labels, busy/error and the pending attempt's revision,
application ID and action ID. Existing `client.query`, `client.selected` and
`client.result_count` continue to describe the underlying application results.
The action inspector omits commands, arguments, source filenames, directories
and activation tokens. Do not collect real catalog labels or queries in shared
acceptance artifacts; the native harness uses only owned synthetic fixtures.

## Validation scope

Source tests exercise query/selection retention, keyboard and secondary-hit
entry, narrow-row reachability, exact descriptor retirement, stale captured
presses, bounded attempts and task-local failure/completion ordering. Two visual
fixtures cover the ordinary and narrow scrolled chooser at both render scales.
The owned native fixture/harness separately tests real input and native action
dispatch. Added tests and helpers are not evidence of execution; executed
checks and native acceptance belong in the source-specific report.

Source `73577a4cbf7aba68ee1c40b1280c27e738a5e820` passed owned native keyboard,
secondary-click, literal-argument, stale-descriptor and native-denial checks.
It also fixes stationary-cursor search clicks after compact/full dock reopening
through explicit framework native-lifetime renewal. Five real compositor capture
cycles contain 710 frames with no compact-at-top signature. Exact preferences
and wallpaper remain unchanged, and fixture resources are removed. See the
[source-specific acceptance report](../reports/measurements/lucent-application-actions.json)
for scope, tests and the remaining interoperability and pacing acceptance.
