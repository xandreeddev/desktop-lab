# Native Bluetooth service

`BluetoothPort` describes observations and commands in the pure domain.
`BlueZ` implements that port in `lucent-services/src/bluetooth/`. The independent
`bluetooth-service` executable is a consumer of this library. It does not load
the desktop, Wayland runtime, renderer, Omarchy picker or `bluetoothctl`.

This milestone supplies the service foundation. The desktop composition root
does **not** inject it yet. Native pairing prompts and the Lucent Bluetooth
panel are still required; the existing desktop entrypoint remains unchanged.

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

`BluetoothAdapter` separates global `discovering` from `discovery_owned`: another
client can be scanning while Lucent owns no discovery session. Device metadata
separates paired, optionally bonded, trusted, blocked, connected and
services-resolved state. Absent battery/RSSI/icon metadata stays `None`; RSSI
remains native dBm rather than an invented percentage. An empty ready catalog
differs from an unavailable daemon or malformed observation.

The pure `validate_bluetooth_command` use case checks current target and parent
identities. Discovery needs a powered controller. Connection needs a powered
controller and a paired, unblocked device. Setting trust requires pairing;
clearing trust does not. Connecting an unpaired device is deliberately rejected
until native pairing exists, because it could otherwise invoke another
application's default agent.

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
5. Another worker calls `control(command)`. A bounded queue wakes the owning
   thread, which validates current state, resolves the handle and invokes native
   methods or property setters. It keeps draining signals while awaiting a
   method reply, so a full signal stream cannot stall the reader behind that
   reply. Never call `control` synchronously inside the watch callback: that is
   the command owner's thread.
6. The returned result acknowledges the request. Later snapshots establish
   observed state. Timeouts do not undo service-side effects; the adapter closes
   the uncertain connection, retires its handles and reconnects without replay.
7. Cancellation closes command admission and explicitly closes the unique
   connection, even while signal streams hold connection references. A second
   simultaneous observer is rejected. The idle timer bounds cancellation; it
   does not poll Bluetooth state. Unavailable service retries wait two seconds.

The current implementation admits 32 waiting commands, 128 queued BlueZ signals,
eight owner-change signals, 32 controllers and 512 devices. A raw object catalog
over 4096 entries is rejected. Refreshes and commands have five-second absolute
deadlines, including command queue time. These are admission/catalog limits,
**not** a total memory budget: zbus must receive and deserialize messages first,
and metadata allocations, daemon memory and simultaneous copies remain outside
those counts. The client must keep its callback responsive.

## Operations and authority

`SetPowered` changes one controller. `SetDiscovery` acquires/releases this
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
adapter installs no permissive policy rule and registers no automatic pairing
agent. A native agent must handle explicit PIN/passkey/confirmation requests,
authorization, cancellation and owner changes before pairing becomes a client
feature; consult the [agent API](https://bluez.readthedocs.io/en/latest/agent-api/).

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
{"command":"connect","device":2}
{"command":"trust","device":2,"trusted":true}
{"command":"disconnect","device":2}
{"command":"forget","device":2}
```

Use IDs from the current detailed snapshot, not these illustrative values. The
consumer is a local library example, not a new public control daemon. The
`acknowledged` event is distinct from a subsequent `state` event. Invalid JSON
and oversized lines fail explicitly. Pairing credentials are not accepted in
this milestone.

Private-daemon protocol tests exercise real D-Bus serialization and method/signal
routing against an owned fixture. They do not contact the host system bus or
prove physical Bluetooth interoperability. Coverage includes discovery
ownership, power/paired controls, metadata removal, identity retirement and
owner replacement, signal pressure, timeout recovery, safe error translation,
unrelated-signal idle behavior, connection cleanup, and absent versus empty
versus disconnected service state.

Native pairing and the desktop panel, physical devices and profiles,
suspend/resume, incoming authorization and hotplug acceptance remain open. The
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
