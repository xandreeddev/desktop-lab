# Native network service

`NetworkPort` is the pure domain contract. `NetworkManager` is its native D-Bus
adapter in `lucent-services/src/network/`. The independent `network-service`
example consumes that contract without the desktop component, renderer, Wayland
event loop, Omarchy menu or `nmcli`.

The composition root injects the same adapter into the native Network child.
Click the network icon or run `lucent-cli network open`. Its Wi-Fi, Saved and
Active tabs offer scanning, visible/hidden joins, saved activation, targeted
disconnect and confirmed deletion. Omarchy network entrypoints open that child;
no terminal wizard is launched. The view borrows snapshots and calls no OS APIs.
One cancellable root subscription lifts observations into messages. An ordered
effect queue moves owned commands to the port. Acceptance is separate from the
observed connection or scan completion; no successful state is invented.

## Masked input and client lifetime

`Element::masked_input(count, placeholder, on_edit)` stores a character count,
not a value. Layout paints bullets and emits Insert, Replace, Backspace or Clear
InputEdit deltas. It never accumulates the full secret in a framework input
buffer or hit value. `on_submit(message)` emits Enter separately from pointer
focus. Tab/Shift+Tab use existing focus traversal and scroll reveal. The generic
control contains no network concepts.

The generation-bound private form owns a zeroizing Secret. Submit consumes it
into non-cloneable credentials and a command. Close, cancel, changed security,
retired targets and service loss clear or drop the form. Stale edits are rejected.
Passwords are absent from elements, inspector state, preferences, argv and retry
snapshots. Failed joins need fresh credentials. Individual event strings and
native transport copies are not guaranteed to be zeroized.

Closing releases keyboard/pointer input immediately; a token-timed exit motion
precedes unmounting. Reopening rejects stale close timers and retargets current
motion. The root coordinates launcher/audio/network keyboard ownership. Closed
network observation continues without an idle redraw loop or nmcli polling.

## Saved-password recovery

A saved profile is activated with its current `NetworkObjectId`. If NetworkManager
actually requests a new WPA-personal or SAE password, the adapter's SecretAgent
publishes a `NetworkCredentialRequest`. The desktop opens the existing token-styled
masked form for that exchange. **Continue** consumes the password into
`NetworkPort::answer_credentials`; it does not call `Join` or
`AddAndActivateConnection2`. The response belongs to the same saved connection's
native `GetSecrets` invocation, preserving its UUID and settings.

The domain request contains only its transient `NetworkCredentialRequestId`,
opaque saved-profile key, name, original SSID octets, security and whether a new
secret was requested. D-Bus paths, sender names, callback senders and settings
serialization remain private to the adapter. `watch_credentials` is a separate
observation lane so an activation awaiting its native reply cannot block the
human prompt. It observes an in-memory coordinator; it opens no second bus
connection. The desktop answers on its existing controls work queue while the
ordinary network queue may still be awaiting activation.

`NetworkManager::with_secret_agent()` explicitly enables registration. The
production desktop composition root opts in; a plain independent service observer
does not. Registration uses NetworkManager's AgentManager and the required native
SecretAgent path on the **same** captured system-bus connection. Only its captured
unique NetworkManager owner may invoke callbacks. A request must match a currently
observed saved profile's native path, UUID and raw SSID. Unsupported requested
settings/hints, noninteractive requests, WPS, enterprise/VPN methods and
agent-owned secret flags are refused. The adapter provides no secret vault and
does not pretend to persist agent-owned passwords. System-owned secret storage
remains NetworkManager's authority; no "Remember" override is presented during
saved-profile repair. Not-saved profiles retain their existing storage policy.

At most eight requests are admitted and each callback expires after 90 seconds.
The consuming response validates its exact live ID and security. Cancel/Escape,
closing or changing the form sends an explicit cancellation; a late prompt after
closing the pending saved activation is declined without reopening the panel.
NetworkManager's `CancelGetSecrets`, saved-profile retirement, Wi-Fi removal/radio
disable, service loss and observer shutdown retire pending callback senders and
drop any editing secret. A replacement callback gets a new ID; stale edits and
answers cannot target it. Response delivery acknowledges only the answer, not
successful association, DHCP or durable storage. There is no automatic credential
retry, silent profile replacement, terminal wizard, or invented connected state.

The form and domain `WifiCredentials` retain zeroizing owned storage through
response serialization. Native incoming and outgoing D-Bus buffers and
NetworkManager's own copies/storage have independent lifetimes and are not
claimed to be zeroized by Lucent.

## Values and ownership

- `NetworkObjectId` is a temporary handle allocated by one adapter. Device,
  access-point, saved-profile and active-connection handles are distinct. A
  removed object or a new service connection receives a new handle even when its
  backend path is reused. Removing a device also retires dependent access points
  and active connections. Do not save these IDs or turn them into D-Bus paths.
- `NetworkProfileKey` is the saved profile's stable key. NetworkManager supplies
  its UUID; the domain treats it as an opaque bounded value. It can identify a
  preference but does not authorize replaying a command to a replacement handle.
- `NetworkSsid` retains 1–32 original octets. Non-UTF-8 and control octets can be
  present. `display_name()` gives safe lossy text; it is never the input to a join
  request. An unknown hidden name is `None`, not an empty validated SSID.
- `NetworkSnapshot` contains devices, saved profiles, visible/hidden access
  points and active connections, plus radio, connectivity and primary-connection
  observations. Credentials are absent. A device's scan generation changes
  after completion; a successful Scan command alone is not completion.
- `WifiCredentials` consumes a zeroizing `Secret`; neither credentials nor
  `NetworkCommand` supports Clone or Debug. Open/OWE requests have no password.
  WPA personal validates a printable ASCII passphrase or a hexadecimal PSK;
  SAE accepts a bounded nonempty string. Enterprise/WEP setup requires an
  existing configured profile in this version. Do not place a password in an
  element tree, inspector, preference, log, command line or retry snapshot.

## From a consumer to the service

1. The consumer constructs `NetworkManager::default()`, normally stores it in an
   `Arc<dyn NetworkPort>`, and starts one worker calling `watch(callback, stop)`.
   There is no implicit global runtime or registration side effect.
2. That worker connects to the **system bus** through zbus. It registers bounded
   event and service-owner streams before reading state, resolves the unique
   NetworkManager owner and rechecks it. Calls target that unique owner, so a
   name replacement cannot silently redirect an in-flight request.
3. An initial bounded query reads properties and saved `GetSettings` metadata.
   Observation never invokes `GetSecrets`. Backend paths stay in a private
   typed identity map; the callback receives only domain values.
4. NetworkManager signals wake the worker. It coalesces state refreshes, handles
   removal notices before publishing IDs and suppresses unchanged snapshots.
   If an object disappears during a multi-call query, its handle is retired and
   the query retries within the same deadline; unrelated handles remain valid.
   GDBus can return `UnknownMethod` for these removed objects. Only known reads
   of dynamic objects treat that response as removal; unsupported control or
   root methods still fail, and repeated query failures exhaust the deadline.
   The idle timer bounds cancellation; it does **not** poll network state.
5. A caller on another effect/worker thread consumes `NetworkCommand` into
   `control`. A bounded channel wakes the owner. The owner resolves current IDs,
   validates device/security intent and makes native methods/property changes.
   It continues draining signals while awaiting a reply; otherwise a full
   signal queue could stall the connection reader and its method reply.
6. The result acknowledges service acceptance. A later snapshot establishes
   connecting, connected, needs-authentication or failed state. Timeout or
   disconnect can follow an accepted operation; it cannot be undone by dropping
   the local future. A transport timeout retires the connection and IDs before
   reconnection. Never offer silent retry against replacement IDs.
7. Cancellation closes observation and command admission. The guard also cleans
   up after a callback panic. A second simultaneous observer is rejected. Do not
   call `control` synchronously from the watch callback: it is running on the
   command owner's thread.

The ordinary service uses at most 32 waiting commands, 128 queued network signals, eight
queued owner-change signals, 64 devices, 512 access points, 512 profiles and 64
active connections. A state refresh and a command have a five-second absolute
deadline, including command queue time. Exceeding a catalog bound is explicit
unavailable state, not a truncated list. Reconnect waits two seconds and respects
StopSignal. The bounds do not cap zbus's incoming message allocation or daemon
memory. Observed state is not a server-wide atomic transaction.

## Operations

`SetWifiEnabled` changes the Wi-Fi radio rather than globally disabling network
connectivity. `Scan` requests discovery. `Activate` uses a current saved-profile
ID and an optional device ID. `Disconnect` targets one active connection.
`Forget` deactivates observed connections using a profile and then deletes it;
these operations are ordered but not atomic. A failure can leave a profile
disconnected and still saved.

`Join` accepts a visible access-point ID or a device/raw-SSID hidden target,
validated credentials and a remember flag. The adapter checks the observed
access point's security before sending settings. Remembered profiles persist to
NetworkManager's disk store. Temporary profiles use its volatile lifetime and
are removed on disconnect. D-Bus/zbus and NetworkManager serialize and may store
credentials according to their own policy: zeroizing the domain value does not
erase those external copies. Named method errors are translated without their
untrusted text bodies, which could echo caller settings.

## Test the library separately

```sh
cargo test --manifest-path lucent/Cargo.toml --locked -p lucent-domain network
cargo test --manifest-path lucent/Cargo.toml --locked -p lucent-services network
cargo run --manifest-path lucent/Cargo.toml --locked -p network-service -- --observe 10
```

The observation command emits redacted counts and state. `--stdio` is an explicit
local test consumer: it emits interface/SSID/profile metadata and accepts JSON
commands over stdin. It is not a public network-control daemon. A numeric
`request_id` correlates a result; secrets are accepted through stdin, never argv.
Its credential-bearing input line is bounded and zeroized. Close stdin to stop
the subscription.

Authorization belongs to NetworkManager/polkit. An SSH process can be denied
even when the same account has an active graphical desktop. The native harness
uses a temporary bounded user service, matching the desktop's process context,
and verifies explicit permission-denied results separately. It does not install
a permissive authorization rule or run the adapter as root.

Protocol tests start private dbus-daemon services and do not contact the host
NetworkManager. They cover raw SSIDs, hidden targets, scan/activation
acknowledgement versus completion, radio changes, disconnect/forget, removal
and path reuse, unique-owner replacement, idle behavior, signal bursts, command
bounds, timeout recovery, cancellation and safe error translation.

`vm/test-network-service.py` exercises the real guest service with owned
mac80211_hwsim radios, hostapd and DHCP in an isolated AP namespace. Its fixture refuses existing Wi-Fi
devices/module state and requires the prepared virtual Arch guest. It preserves
Ethernet, original profiles/radio state and the running desktop. Build with the
VM off, boot only Lucent and use a private login file for fixture sudo commands:

```sh
cargo build --manifest-path lucent/Cargo.toml --locked --release -p network-service
python3 vm/test-network-service.py --login-file /path/to/private/LOGIN.txt
```

Actual test results belong in the milestone's measurement report. Physical Wi-Fi,
enterprise provisioning, SAE/OWE interoperability, captive portal UX and suspend remain separate hardware
acceptance work. Secret-agent implementation checks and the extended native panel
recipe below have separate evidence; implementation alone is not native proof.

The [service evidence](../reports/measurements/lucent-network-service.json) records
the tested binary and simulated-radio workflow: scan, WPA2/hidden association,
DHCP, disconnect/reactivation/forget, radio changes, removal and owned cleanup.
It does not establish a native desktop panel or physical-hardware compatibility.

Protocol references: [NetworkManager](https://networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.html),
[wireless device](https://networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.Device.Wireless.html),
[wireless security](https://networkmanager.dev/docs/api/latest/settings-802-11-wireless-security.html).

## Native panel evidence

The prepared guest passed real pointer/keyboard masked input, Tab/Shift+Tab/Enter,
visible and hidden WPA2 joins and DHCP, saved activation, targeted disconnect,
confirmed Forget, volatile-profile cleanup, radio toggles, compatibility entrypoints
and surface release. Existing profiles, routes, wallpaper and the desktop process
were preserved. See [the panel report](../reports/measurements/lucent-network-panel.json).

The two-minute warm idle sample measured 225.33 MiB mean process RSS and 0.567%
of one logical core. This checkpoint is within the current 300 MiB normal-desktop target; software-driver mappings
can contribute to RSS and separate GPU allocation was not measured. That historical panel checkpoint did not test saved-password SecretAgent
recovery. Physical Wi-Fi, SAE/OWE/enterprise, captive portals and suspend remain
unverified by that checkpoint. This report does not establish a complete control center.


## Credential recovery validation

The focused private-bus tests exercise an activation deliberately held open until
its actual `GetSecrets` response, proving the separate answer lane can complete
without another join or saved-profile identity. Additional protocol cases cover
unauthorized senders, noninteractive/unknown flags, unsupported hints/security,
agent-owned vault requests, UUID mismatch, daemon cancellation, profile removal,
service-owner loss and stale one-shot response rejection. Client tests cover
masked consuming submission while ordinary activation is pending, close-before-
acknowledgement, late prompts, daemon withdrawal, replacement generations and
credential observation arriving before the first network snapshot.

The extended `vm/test-network-panel.py` recipe owns its hwsim profile and refuses
to alter original profiles. After initial WPA2/DHCP and saved reconnection it:

1. Disconnects only its owned station and installs a deliberately incorrect test
   password into its registered fixture profile.
2. Waits for the real native SecretAgent form, enters a disposable fixture value,
   checks inspector redaction and cancels with Escape.
3. Clears only that fixture's system-owned password, reconnects, enters the real
   fixture credential through verified native masked input and submits with Enter.
4. Requires the original UUID, one matching profile, real association and DHCP.
5. Checks the daemon's stored secret inside the root fixture process, emitting
   only a boolean comparison, then disconnects/reconnects without another prompt.
6. Restores original profiles, routes, radio state, wallpaper and desktop process
   through the existing fixture cleanup guards.

`python3 -m py_compile vm/network_fixture.py vm/test-network-panel.py` validates
helper syntax only. Cargo, private-bus, visual and real-guest results must be
recorded from their actual executions; this source change does not by itself
establish any of those acceptance results.

Protocol references for this slice:
[SecretAgent](https://networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.SecretAgent.html),
[AgentManager](https://networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.AgentManager.html)
and [secret flags](https://networkmanager.dev/docs/api/latest/nm-dbus-types.html).
