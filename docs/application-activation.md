# Standard application activation

An application can now declare a command or standard desktop-service activation.
`ApplicationLaunch::Command` carries its parsed argument vector.
`ApplicationLaunch::DesktopService` carries an optional executable fallback.
A service-only `.desktop` entry is no longer discarded because it lacks `Exec`.
There is no invented executable sentinel.

`ApplicationPort` remains the domain contract. `XdgApplications` is its supplied
adapter; the launcher and dock consume that port. Existing pure window-class
matching and prefer-running focus behavior remain unchanged.

## Native behavior

The adapter reads `DBusActivatable=true` and derives the well-known D-Bus name
from `AppId`, removing `.desktop`. It validates the native name and constructs the
object path by replacing dots with slashes and dashes with underscores. It invokes
`org.freedesktop.Application.Activate` with ordinary typed D-Bus arguments, never
an application-generated command string. See the
[freedesktop activation specification](https://specifications.freedesktop.org/desktop-entry/latest/dbus.html).

Native connection, method, close and permitted command fallback share one
five-second dispatch budget. Close gets at most 250 ms within that budget.
The connection's local executor is driven while the request waits; it creates no
permanent polling worker.
Native errors are classified without displaying their raw bodies.

A definitive missing service/object/method permits the parsed `Exec` fallback.
A denied call or uncertain reply timeout returns failure and starts no second
instance. Command fallbacks still use independent user application units and
literal argument arrays. Restarting Lucent does not put those applications inside
its shell service lifetime.

## Fresh user intent

The SDK's opt-in `Effects::activation_token` requests an opaque token for the current
physical pointer/key dispatch. The Wayland adapter binds `xdg_activation_v1`, captures
the actual seat, serial and initiating surface, then commits one asynchronous request.
No retained input serial or inherited `XDG_ACTIVATION_TOKEN` supplies authority.
There are at most eight pending requests and a two-second reply deadline. Secure
clients keep activation disabled. Unsupported protocols and IPC commands without a
physical input report an explicit outcome; ordinary launching can still proceed.

The desktop captures the selected `Application` and maps the SDK completion into an
`ApplicationLaunchContext`. It rechecks catalog authority before scheduling the port.
The domain context contains an optional checked, opaque token, with redacted Debug;
it has no Wayland handles. Production `XdgApplications` implements
`launch_with_context`, `launch_action_with_context` and `open`.

D-Bus `Activate`, `ActivateAction` and `Open` receive the fresh value under the
`activation-token` platform-data key. A command launch passes it only in the
`systemd-run` child's environment and imports it by name into the independent user
application unit. The token never appears in argv, and the global process
environment is never mutated. Without a fresh value, old activation/startup tokens
remain excluded. The compositor decides whether to focus the application; receiving
a token is not proof that focus or presentation occurred.

The native protocol is documented in the upstream
[xdg-activation XML](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/staging/xdg-activation/xdg-activation-v1.xml).

## Explicit files and URIs

`ApplicationOpenTarget` distinguishes checked absolute local paths from explicit
application URIs. Explicit application targets support valid URI schemes such as
`smb` or `steam`; this does not widen the separate notification-link policy.
`ApplicationOpenRequest` admits 1–64 targets with a 128 KiB
encoded-content bound. It performs no filesystem or transport work. The selected
application stays explicit: failure never silently opens a different default handler.

For a desktop service, `Open` receives the URI array and optional activation data.
Definitive unsupported service/object/method errors permit only that application's
current command fallback. Denial, uncertainty and timeout never repeat the request
through another process. `XdgApplications` freshly resolves the desktop source under
normal priority/masking rules and rejects changed captured descriptors.

For a command fallback, the adapter re-expands its current `Exec` template. `%F` and
`%U` preserve a list as separate arguments; `%f` and `%u` create separate invocations
for multiple targets. One shared five-second dispatch budget bounds the request;
an error can follow earlier successful invocations, so it is never automatically
replayed. Only the first invocation receives the intent token. Name, icon and
desktop-file fields expand exactly once. File
fields accept local paths/file URIs; they explicitly refuse remote resources rather
than claiming to download them. URI fields retain URI encoding. Quoted field codes,
multiple file/URI fields and multi-argument fields embedded inside another argument
are rejected. No shell interprets target contents.

Inside the graphical session:

```sh
lucent-cli applications open org.example.Editor.desktop '/absolute/a b.txt'
lucent-cli applications open org.example.Browser.desktop 'https://example.org/'
```

The CLI encodes each argument as a separate JSON value on the existing private
command socket. The command protocol retains its 4000-byte limit. Its reply admits
work; an actual native failure arrives through desktop error state. The independent
`application-service --open APP_ID FILE_OR_URI ...` consumer demonstrates the same
port without a renderer or Wayland dependency.

The independent graphical `application-intent` example demonstrates the other
boundary: a physical button dispatch requests the SDK token, maps its completion
into a domain launch context, then invokes an injected `ApplicationPort` on a
worker. Its composition root supplies `XdgApplications`; its view performs no OS
calls. It can launch, open the supplied targets or invoke one declared action:

```sh
cargo run --locked -p application-intent -- \
  --app org.example.Editor.desktop --target '/absolute/a b.txt' \
  --action NewWindow
```

Omit `--action` when the application does not declare one. `--data-dir` selects
an isolated XDG catalog for acceptance. The example's inspector exposes only
busy/completion counters and a boolean indicating fresh authority. Its socket
`open` command exercises the same port without physical input and must not reuse
the preceding button's serial or token. An acknowledged operation still needs
verification in the target application; the example does not equate dispatch
with observed focus or file loading.

## Verification scope

Declared desktop actions predate this milestone and already have owned native
acceptance; see [application-actions.md](application-actions.md). The new token,
Open, parser and observation changes require their own final strict, protocol,
visual and native application acceptance. Do not treat source implementation as
proof of broad third-party compatibility. Current evidence and remaining checks
are tracked in `docs/daily-use-completion.json`.

`vm/test-application-intent.py` prepares an owned desktop entry and private real
Chromium profile, then exercises physical launch/open/action and socket/CLI
opening. The fixture records token **presence**, observes owned window/focus
state, and checks actual file URI/title loading through that profile's loopback
DevTools endpoint. Child-environment delivery and observed focus do not prove
that Chromium consumed a specific activation protocol message. The run requires
an identified release containing the independent example, an unlocked idle
single-output VM and bounded owned-process cleanup. Its existence is not an
executed result; no native pass is claimed here.
