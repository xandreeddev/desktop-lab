# Native notifications: delivery, history and client state

Lucent owns `org.freedesktop.Notifications` and renders notification UI with its
normal framework. Omarchy notification shortcuts route to this client. The
compositor places one ordinary overlay surface; it does not create the cards,
groups, buttons or animations.

## The boundaries

| Contract | Responsibility | Supplied adapter/client |
| --- | --- | --- |
| `NotificationPort` | Observe snapshots; dismiss/invoke a current version; DND, clear and retry | `FreedesktopNotifications` |
| `NotificationHistoryPort` | Load/save a bounded domain archive | `JsonNotificationHistory` |
| `NotificationInbox` | Receive/replacement IDs, versions, timeout, retention and archive policy | Pure use case; no files, bus or renderer |
| `notifications::Center` | Group choice, paging, scrolling, action paging, service errors and reveal motion | Desktop child component |
| `notifications::view` | Token-based cards, grouped history and controlled scrolling | Generic `Element` tree |
| `lucent-wayland` / `lucent-render` | Route input, pace dirty frames, prepare and submit scene paint | Generic engine; no notification identities |

Production adapters are selected in `platform::desktop_adapters()`. The desktop
passes its notification adapter to `Center`; the delivery adapter gets the
separate history adapter. Tests supply implementations of those ports. Neither
the client view nor the pure inbox reads a file or performs a D-Bus operation.

The independent `notification-service` example consumes the same domain port,
without a desktop-client, compositor or renderer dependency. It can provide the
native server while the desktop notification server is stopped. It refuses to
replace another owner. Its output contains counts and IDs, never notification
body text. Use `--help` for commands and `--memory` for an ephemeral archive.

## How an incoming notification becomes pixels

1. An application calls the standard `Notify` D-Bus method. The native adapter
   bounds its UTF-8 strings/actions and reads supported hints, including the
   optional desktop-entry identity, urgency, resident and transient flags.
2. The adapter admits any required close signal before mutating state. The pure
   inbox assigns a wire ID for new notices and a new monotonic revision for every
   receive, including replacement. Timeouts use its supplied monotonic clock;
   receipt metadata uses the adapter's supplied Unix time.
3. The delivery worker submits the current archive to one ordered storage
   worker. Its mailbox holds one pending load and the latest pending save, in
   addition to one active call. Repeated updates replace pending saves. Storage
   holds neither the inbox mutex nor the native connection while it writes;
   snapshots, action/close signals and expiration keep progressing.
4. A snapshot reaches the desktop through its existing bounded subscription
   delivery. `Center::update` keeps a stable selected group where possible and
   retires action-paging state for replaced versions.
5. `Center::view` declares a tree. Layout produces text/card geometry, paint,
   clipped hits and the controlled scroll viewport. The native backend applies
   input regions and submits the renderer's Vulkan work. There is no browser or
   third-party shell in this path.

The adapter currently wakes for mutations and checks expiry/disconnection on a
100 ms bounded wait. It does not emit an unchanged snapshot on each check. This
is distinct from continuous rendering; idle notification surfaces repaint only
when state or an active client animation requires it.

## Current actions have identity

The sender's `u32` wire ID is replaceable. It is insufficient for a callback
captured by an old frame. `NotificationTarget::new(id, revision)` rejects zero
values; every active notification exposes its current target. Client buttons
capture that target, and both the child and adapter check the current version
before queuing/executing an action.

Replacement preserves the wire ID but changes the revision. A stale button
cannot dismiss or invoke the replacement. Unknown action keys are rejected;
accepted keys are returned unchanged in `ActionInvoked`. A non-resident action
requires room for both action and close signals before changing the inbox.
Resident actions keep the notification active. Admission/queueing is not proof
that the originating application handled the signal.

On a bus disconnect, active transport identities retire into ordinary history
and pending transport signals are discarded. The observer receives the retired
snapshot followed by the unavailable state. Reconnection never replays old
application actions. Historical entries have no executable action controls.

## Durable history and recovery

The production file is `$XDG_STATE_HOME/lucent/notifications.json`, normally
`~/.local/state/lucent/notifications.json`. It is a versioned, bounded JSON
archive. New directories use mode 0700; replacement files use mode 0600. A private
advisory lease serializes file reads/writes across adapter instances, so a later
instance cannot restore a counter before an earlier in-flight writer finishes. Writes
use an exclusively created temporary file, synchronization and an atomic rename.
The final read rejects symbolic links and non-regular files.

The inbox admits at most 64 active notices and keeps at most 100 closed entries.
A durable archive retains at most 100 recent non-transient entries, including
still-active notices. A restarted server restores those as history; it cannot
resurrect transport authority or promise the old application still exists.
DND and the revision counter survive alongside the archive. Stored body text is
limited to 4 KiB, with bounded metadata/actions and a 4 MiB file-read limit.
Transient notices do not enter the archive.

The initial load completes before native sender admission, preserving the
persisted revision counter. This wait is cancellable and owns no bus name. Later
recovery loads run while delivery continues. Loading/saving are separate
observations, and only the latest matching operation may update those states.
Clear invalidates pending load replies before its queued save, so a late recovery
cannot restore discarded history. An already live saved entry is not duplicated
when merging.

Read failures preserve the existing file and disable automatic replacement.
Delivery continues in memory and the history panel shows the failure. **Retry**
reads again and merges recovered history without replacing live versions or
newer local DND choices. **Clear** is an explicit request to discard history and
permits replacing an unreadable file; current active notices remain intact.

Write failure retains live content and reports its storage state. A subsequent
content change or explicit Retry can attempt saving again. The adapter does not
spin on a failed write. Completion of a clear/retry command is admission; observe
the storage state for save completion. Filesystem calls are not forcibly
cancelled: retiring an adapter discards queued requests without blocking its
caller, and an already active call may finish. Its worker owns no native
connection and holds only a weak completion reference, so it cannot resurrect
delivery or UI state. The file lease serializes a later instance with that call.
There is no automatic shutdown flush promise; observe Ready for a current save
before relying on its completion. Physical filesystem faults and power-loss
behavior still need separate acceptance.

Notification content stays in this private state file and is not included in
public reports or application arguments. The history is not encrypted.

## Grouped client and animation

Groups prefer a valid desktop-entry identity; otherwise the supplied application
name is their key. The child owns the selected group and note page. A group row
opens its details, and selecting it again collapses it. Previous/Next stays
within that group's entries. All groups/details use the SDK's controlled scroll
API, including wheel, thumb capture and keyboard reveal.

Toasts retain compact bodies. History details allow a larger bounded body layout
inside the scroll viewport. The design system supplies margins, radii, padding,
row and panel sizes, typography, colors and motion curves. Storage policy values
are not visual design tokens.

Opening/closing uses the framework's `Motion` with the client's supplied time.
`Desktop::animating` requests compositor-paced work only while it is active.
Reversing an unfinished transition starts from its current value. Closing keeps
the surface until a settled `FrameSubmitted` message retires the closing state.
That message means successful submission, not GPU completion or scanout. No
extra notification animation timer or framework effect worker is created. The
separate native storage worker sleeps on its mailbox when idle.

## Validation and remaining work

Owned private-bus cases cover saturation/admission, exclusive ownership,
disconnection/reconnect, replacement/action/expiry, archive restart, corrupt-file
preservation, explicit clear/recovery, save failure/retry and method admission
while a storage writer is stalled. Further private-bus cases require actual
snapshot, action, expiration, disconnect/reconnect and name-release progress
during that stall, plus live delivery during a paused recovery load. Queue tests
exercise a thousand superseding saves and blocked-call retirement. Pure tests
cover versions, archive validation,
retention and merging; injected client tests cover grouping, paging, effects and
animation continuity. Render references cover both palettes at 1x/2x.

Real VM/compositor keyboard and pointer acceptance, restart/sleep behavior and
physical filesystem/performance measurements remain deferred. The server still
advertises `body` and `actions`. Persistent *history* does not establish the
protocol's optional `persistence` guarantee, so that capability is not claimed.
Formatted markup, hyperlinks, images/icons, activation tokens, sound and richer
notification interactions remain implementation work. See the
[notification specification](https://specifications.freedesktop.org/notification/latest-single/)
and the complete ledger in `docs/implementation-status.json`.
