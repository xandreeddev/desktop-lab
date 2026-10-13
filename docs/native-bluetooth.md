# Native Bluetooth service

`BluetoothPort` describes observations and commands in the pure domain.
`BlueZ` implements that port in `lucent-services/src/bluetooth/`. The independent
`bluetooth-service` executable is a consumer of this library. It does not load
the desktop, Wayland runtime, renderer, Omarchy picker or `bluetoothctl`.

The desktop composition root injects this adapter into a native Rust Bluetooth
child. Pairing conversations and token-based views are implemented. Native
bar/keyboard/IPC surface checks and actual ready-empty/unavailable daemon
observations pass. Two owned virtual HCI controllers also pass native radio
controls, discovery, explicit comparison-code pairing, bonding and trust removal.
That run exposed a signal-filter bug: external controller/device property changes
were not refreshing the panel. Its native-adapter fix still needs a rebuilt-guest
check. Installed compatibility entrypoints remain unchanged until that acceptance
is established; physical device/profile interoperability remains unverified.

## Values, identity and policy

`BluetoothObjectId` is a nonzero, adapter-instance handle for a currently observed
controller or device. Removal retires it; controller removal also retires its
dependent devices. Reusing a D-Bus path or replacing the service connection
allocates new IDs. Do not persist IDs or derive them from an address.

`BluetoothAddress` validates six octets and displays uppercase colon-separated
hexadecimal. It describes an observed address, not a universal stable device
identity: privacy and address types need consideration before adding remembered
device preferences. Controller/device snapshots are public records, not sealed
value objects. The adapter validates required metadata, optional types and
battery percentages before publishing them.

`BluetoothAdapter` names the physical radio-controller domain record. It differs
from the architectural adapter: `BlueZ` implements `BluetoothPort`. The radio
record separates global `discovering` from `discovery_owned`: another
client can be scanning while Lucent owns no discovery session. Device metadata
separates paired, optionally bonded, trusted, blocked, connected and
services-resolved state. Absent battery/RSSI/icon metadata stays `None`; RSSI
remains native dBm rather than an invented percentage. An empty ready catalog
differs from an unavailable daemon or malformed observation.

The pure `validate_bluetooth_command` use case checks current target and parent
identities. Discovery needs a powered controller. Connection needs a powered
controller and a paired, unblocked device. Setting trust requires pairing;
clearing trust does not. Connecting an unpaired device is deliberately rejected
because connection and pairing are distinct operations. `start_pairing` validates
an unpaired, powered, unblocked target before the explicit pairing operation.

`BluetoothPairingId` identifies one transient attempt or request, not a device.
`BluetoothPairingCode` validates the six-digit numeric range and redacts Debug;
`digits()` is an explicit presentation operation. Entered PIN/passkey answers
consume non-Clone, non-Debug wrappers around zeroizing `Secret` storage. PINs
contain 1–16 ASCII letters/digits; entered passkeys contain 1–6 ASCII digits.
`BluetoothServiceId` validates a canonical UUID, never an executable command.
Prompts/attempts remain public domain records, not sealed value objects.

## What starts and what owns the loop

1. A consumer explicitly creates `BlueZ::default()` and stores it behind
   `Arc<dyn BluetoothPort>`. One worker calls `watch(callback, stop)`. Construction
   alone starts no threads or global registry.
2. That worker connects through zbus to the **system bus**, sets up bounded
   signal and service-owner streams, resolves BlueZ's unique owner and rechecks
   it. Native calls target that unique owner, so a replacement daemon cannot
   silently receive an old command.
3. `GetManagedObjects` reads the initial catalog. Transport paths remain private
   to the adapter; the callback receives domain records and transient handles.
   Sorting by backend path makes unchanged snapshots compare deterministically.
4. Relevant ObjectManager and PropertiesChanged signals wake the owner. It
   coalesces refreshes, retires observed removals before resolving commands and
   suppresses unchanged snapshots. Battery-interface removal clears optional
   metadata without retiring the device. Unrelated GATT changes do not request
   catalog refreshes. A query crossing a removal is retried within the same
   absolute deadline.
5. Another worker calls `control(command)` or a pairing method. A bounded queue wakes the owning
   thread, which validates current state, resolves the handle and invokes native
   methods or property setters. A private local executor drives at most one
   ordinary method, one owned pairing method and one urgent cancellation method while the owner routes replies,
   callbacks and signals. This allows a user answer to reach a native `Pair`
   call that is awaiting that very answer. Further ordinary writes fail busy;
   they are not silently replayed. Never call port methods synchronously inside
   the watch callback: that is the command owner's thread.
6. The returned result acknowledges the request. Later snapshots establish
   observed state. Timeouts do not undo service-side effects; the adapter closes
   the uncertain connection, retires its handles and reconnects without replay.
7. Cancellation closes command admission and explicitly closes the unique
   connection, even while signal streams hold connection references. A second
   simultaneous observer is rejected. The idle timer bounds cancellation; it
   does not poll Bluetooth state. Unavailable service retries wait two seconds.

The current implementation admits 32 waiting commands, 128 queued BlueZ signals,
eight owner-change signals, 32 controllers and 512 devices. A raw object catalog
over 4096 entries is rejected. Refreshes and ordinary commands have five-second
absolute deadlines including queue time. Connect/default-agent authorization and
an admitted Pair allow 120 seconds; each interactive callback allows 90 seconds.
At most eight agent proposals and eight pending cards are retained in their
separate bounded stages. These are admission/catalog limits,
**not** a total memory budget: zbus must receive and deserialize messages first,
and metadata allocations, daemon memory and simultaneous copies remain outside
those counts. The client must keep its callback responsive.

## Operations and authority

`SetPowered` changes one controller. `SetDiscoverable` changes its global radio
visibility and requires power. Native timeout or another settings client may
hide it again; observe that state rather than mirroring command acceptance.
`SetPairable` changes permission for incoming pairing, including while powered
off. It does not forbid an explicit outgoing Pair, register an agent or trust a
device. The desktop presents these as separate visibility, incoming permission
and request-handling decisions. Their native guest/controller acceptance is
still pending.

`SetDiscovery` acquires/releases this
connection's discovery session idempotently; stopping our scan does not stop
another client's session. Closing the connection releases its owner lifetime.
BlueZ shares discovery across clients. See the
[adapter API](https://bluez.readthedocs.io/en/latest/adapter-api/).

`Connect` and `Disconnect` target a currently paired device; `SetTrusted` changes
its trust flag. `Forget` calls the selected controller's `RemoveDevice` for the
exact selected device, removing its pairing information. Successful Connect
does not mean every profile is connected or service discovery has finished. See
the [device API](https://bluez.readthedocs.io/en/latest/device-api/).

BlueZ and system D-Bus policy remain the authorization authority. Named errors
are translated into safe domain errors without echoing their text bodies. The
adapter installs no permissive policy rule. Observation registers no agent.
An explicit Pair/Connect lazily registers a `KeyboardDisplay` application agent.
Outgoing use does not request the default role; `set_incoming_pairing(true)`
explicitly requests that separate authority. This does not change the radio's
discoverable/pairable flags. Disabling unregisters our agent, cancels pending
cards and permits lazy registration on the next outgoing operation.

The native Agent1 handles PIN/passkey entry, zero-padded display/progress,
comparison, pairing/service authorization, cancellation and release. Only the
observed unique BlueZ owner may call it, and every proposal resolves against a
fresh, current device catalog. Display progress retains one card identity.
Nothing automatically accepts or trusts a device. Epochs retire pending answers
on native Cancel/Release; removal, owner changes and uncertain timeouts retire
the connection. Release rejects new callbacks and invalidates pending requests
immediately. A zbus response-dispatch notifier gates connection retirement until
the Release reply is dispatched; completion of the interrupted operation cannot
erase that retirement intent. This is local response dispatch, not proof of
receipt by BlueZ. The PIN reply owns zeroizing storage through serialization;
this does not promise zeroization of D-Bus, kernel or remote copies. See the
[agent API](https://bluez.readthedocs.io/en/latest/agent-api/).

`start_pairing` returns local admission plus an attempt ID, before human input;
watch the attempt phase and actual paired/bonded metadata for completion.
`answer_pairing` delivers one matching answer to one current request. It does
not itself establish pairing success. `cancel_pairing` targets a live owned
attempt and does not undo a completed bond. Rejecting an owned display card
calls native CancelPairing; an incoming displayed challenge disconnects its link,
because it has already replied void. Neither operation merely hides the UI.

## Run the independent consumer

```sh
cargo test --manifest-path lucent/Cargo.toml --locked -p lucent-domain bluetooth
cargo test --manifest-path lucent/Cargo.toml --locked -p lucent-usecases bluetooth
cargo test --manifest-path lucent/Cargo.toml --locked -p lucent-services bluetooth
cargo test --manifest-path lucent/Cargo.toml --locked -p lucent-services bluetooth_protocol -- --ignored
cargo run --manifest-path lucent/Cargo.toml --locked -p bluetooth-service -- --observe --seconds 10
```

Observation prints only controller/device/connected counts. `--stdio` explicitly
opts into local names, addresses and transient handles, and accepts one bounded
JSON command per line. Close stdin to stop. `--bus-address` selects an isolated
test bus; the default always uses the system bus.

```json
{"command":"discovery","adapter":1,"active":true}
{"command":"visibility","adapter":1,"discoverable":true}
{"command":"pairable","adapter":1,"pairable":false}
{"command":"connect","device":2}
{"command":"trust","device":2,"trusted":true}
{"command":"disconnect","device":2}
{"command":"forget","device":2}
{"command":"pair","device":2}
{"command":"answer","request":4,"answer":"accept"}
{"command":"answer","request":4,"answer":"pin","input":"fixturePin1"}
{"command":"cancel","attempt":3}
{"command":"incoming","enabled":true}
```

Use IDs from the current detailed snapshot, not these illustrative values. The
consumer is a local library example, not a new public control daemon. The
`acknowledged` event is distinct from a subsequent `state` event. Invalid JSON
and oversized lines fail explicitly. Entered credentials belong only on stdin,
not argv/environment. Input lines zeroize on drop; typed decoding moves entered
strings into Secret rather than retaining a generic JSON copy. One bounded
ordinary worker keeps stdin available for replies during an interactive Connect;
EOF stops observation and joins both workers. Detailed mode explicitly reveals
displayed comparison codes, never entered credentials; counts-only mode does not.

Private-daemon protocol tests exercise real D-Bus serialization and method/signal
routing against an owned fixture. They do not contact the host system bus or
prove physical Bluetooth interoperability. Coverage includes discovery
ownership, power/paired controls, metadata removal, identity retirement and
owner replacement, signal pressure, timeout recovery, safe error translation,
unrelated-signal idle behavior, connection cleanup, and absent versus empty
versus disconnected service state. Native Agent1 tests keep Pair pending while
another caller responds/cancels, exercise PIN/passkey serialization, confirmation
and rejection, stable display progress, exact sender/current-target checks,
default-agent lifecycle, late answers, device removal and uncertain timeout
connection cleanup. These are protocol fixtures, not radio interoperability.

Physical controller/profile interoperability, native incoming-controller
authorization, PIN/passkey entry, cancellation, suspend/resume and hotplug
acceptance remain open. The
larger control-center requirement remains in progress in
[the implementation ledger](implementation-status.json).

## Native guest observation

`vm/test-bluetooth-service.py` copies a built release consumer into an owned
temporary guest directory, verifies its hash, queries the real system-bus BlueZ
daemon as the regular user, and tests EOF cancellation. It sends no controls and
publishes counts only. The normal check expects BlueZ to be active already.

The prepared VM has no Bluetooth controller or loaded Bluetooth module. The
explicit fixture options can load that absent prerequisite and start the
inactive daemon; cleanup restores the service, removes only new module holders
(BlueZ can auto-load BNEP), and removes the temporary binary. Existing modules
and arriving controllers are not forcibly removed. No policy/configuration or
installed desktop binary is changed.

```sh
cargo build --manifest-path lucent/Cargo.toml --locked --release -p bluetooth-service
python3 vm/test-bluetooth-service.py
# For the prepared controller-free guest, using its private existing login file:
python3 vm/test-bluetooth-service.py --start-service --load-module --login-file /path/to/private/LOGIN.txt
```

The [recorded service evidence](../reports/measurements/lucent-bluetooth-service.json)
distinguishes the real daemon's empty ready catalog from protocol-fixture device
controls. Neither establishes physical pairing or a native desktop panel.


## Native desktop client and failure ownership

`platform::desktop_adapters()` chooses BlueZ once. `DesktopAdapters.bluetooth`
injects `Arc<dyn BluetoothPort>` into `bluetooth::Bluetooth`; constructing the
child starts no process. The root's `Subscription::stream("bluetooth", ...)`
starts observation and adapts framework cancellation to the domain stop signal.

A changed snapshot becomes a typed child message on the UI thread. The child
owns tab/controller selection, controlled scrolling, panel motion and private
PIN/passkey editing. Views borrow records and use the shared Element, Motion,
masked-input and scroll APIs. They perform no OS operations and borrow only an
entered-character count. The root lifts messages with Element::map and effects
with Effects::delegate. Native surface, keyboard focus and animation scheduling
remain generic runtime concerns.

Ordinary Bluetooth effects use the `bluetooth` lane; prompt answers and
cancellation use the short `controls` lane, independently of a pending Connect.
The client centralizes eight ordered lanes in work.rs instead of exceeding the
runtime's worker bound. `task_on_with_failure` captures the specific operation
revision, so a failure on a shared lane cannot clear another feature's active
request. Delegation preserves that handler and cancellation. Unhandled task
failures retain Application::runtime_failure; mapper panics use a safe fallback.
An uncertain ordinary Bluetooth worker failure restarts the client/owner instead
of guessing that a write failed before execution.

Closing drops entered input, declines unresolved requests, cancels a live owned
attempt and releases our discovery sessions. An admission that finishes after
close is cancelled by its returned attempt ID without reopening the prompt.
Stopping an accepted scan follows it on the same lane even if its owned-state
snapshot has not arrived. Unrelated clients' discovery is preserved.

The device tabs keep selection, power and scanning compact. A separate Radio
tab provides visibility, permission for incoming pairs and Lucent request
handling. The panel includes device pair/connect/disconnect/trust, confirmed
forgetting, explicit incoming-agent authority and
native confirmation/entry/display/service prompts. The top-bar button and
`lucent-cli bluetooth open|toggle|close` use this child. Inspector output excludes
entered input and displayed codes. Radio controls are implemented; their native
guest/controller acceptance, pending Connect cancellation UX, real-device/profile
acceptance and broader power/tray services still need work. The complete control-center requirement remains open.

## Native panel checkpoint

`python3 vm/test-bluetooth-panel.py` checks an already unlocked guest through
real bar clicks, Tab focus, Escape and IPC open/close. It restores the original
panel state and keeps captures private. It performs no radio control or pairing.
The [partial native report](../reports/measurements/lucent-bluetooth-panel.json)
identifies the actual installed binary and distinguishes these checks from
controller acceptance.

The read-only consumer fixture temporarily starts an inactive daemon and loads
its absent prerequisite only with explicit flags. In a logged-in desktop, other
clients can reactivate BlueZ after stop. Cleanup temporarily masks only the
fixture-owned service while unloading new Bluetooth/BNEP modules, then removes
its own runtime mask. Existing overrides/controllers are refused; no force
unload or permanent service policy is introduced. The repeated native run
restores the original inactive daemon and absent modules.

## Owned virtual-controller checkpoint

`vm/bluetooth_fixture.py` runs as root only inside the prepared Arch/KVM guest.
It accepts an explicitly supplied, SHA-256-verified `btvirt` executable, refuses
existing controllers, Bluetooth modules and runtime service overrides, and
journals process birth identities, modules and storage before changing them.
The tested tool comes from BlueZ 5.87, revision
`65d11edee469c24dcc8d075b8be9c040cebaaf98` (LGPL-2.1-or-later);
it creates two software controllers through the guest kernel's virtual HCI
driver. It neither forwards USB devices nor accesses host Bluetooth.

One controller belongs to the native Lucent panel. A separate `bluetoothctl`
process acts as the reference peer's agent, without opening a terminal window.
The test compares the native six-digit card with the peer's actual prompt
before clicking Accept. It checks BlueZ's `Paired`, `Bonded` and `Trusted`
properties rather than treating command admission as successful pairing.
Native discovery, comparison-code pairing/bonding and explicit trust/removal
pass; no device is automatically trusted. Captures and peer output stay private.

The same run caught an observation defect: a `path_namespace='/'` D-Bus rule
received no nested property signals on this guest's system bus, while a rule
limited to BlueZ's unique sender received the power changes. Native control
completion forced a fresh snapshot and hid this problem in ordinary button
tests. The fix removes the redundant path filter in the platform adapter;
interface/member filtering remains in `Notices`. A rebuilt native test is
pending. Polling in the client would bypass the service boundary and would not
repair signal ownership.

Cleanup stops the owned reference/emulator processes, temporarily masks only
the fixture's BlueZ service while unloading new module holders, deletes only
new storage for the two owned controller addresses and removes its runtime
mask. Process identity checks tolerate exit races and refuse recycled PIDs or
missing birth identities. The original inactive service, absent Bluetooth
modules and healthy native desktop were verified after cleanup. Software HCI
pairing does not establish physical radio, A2DP/HID profile or secure hardware
interoperability.
