# Native network service

`NetworkPort` is the pure domain contract. `NetworkManager` is its native D-Bus
adapter in `lucent-services/src/network/`. The independent `network-service`
example consumes that contract without the desktop component, renderer, Wayland
event loop, Omarchy menu or `nmcli`.

The adapter is not yet selected by `platform::desktop_adapters()`. The existing
desktop network menu still uses its compatibility flow, including `nmtui` for new
connections. The following milestone must replace that flow with a Lucent child
component, framework controls, masked secret input and explicit error/focus state.
Adding a port does not establish a usable native network panel.

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

The service uses at most 32 waiting commands, 128 queued network signals, eight
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
enterprise provisioning, secret-agent prompting for saved profiles with missing
credentials, SAE/OWE interoperability, captive portal UX, suspend and the native
panel remain separate acceptance work.

Protocol references: [NetworkManager](https://networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.html),
[wireless device](https://networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.Device.Wireless.html),
[wireless security](https://networkmanager.dev/docs/api/latest/settings-802-11-wireless-security.html).
