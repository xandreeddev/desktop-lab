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

Connection and request work share a five-second deadline. The connection's local
executor is driven while the request waits; it creates no permanent polling worker.
Native errors are classified without displaying their raw bodies.

A definitive missing service/object/method permits the parsed `Exec` fallback.
A denied call or uncertain reply timeout returns failure and starts no second
instance. Command fallbacks still use independent user application units and
literal argument arrays. Restarting Lucent does not put those applications inside
its shell service lifetime.

## Current scope

This path handles the ordinary launch/Activate operation. File/URI Open requests,
desktop actions/ActivateAction, launch-time xdg activation tokens and broader entry
compatibility remain distinct work. Platform data is currently empty; inherited
old activation tokens are not treated as fresh user intent.

Owned private D-Bus fixtures exercise actual calls, success, missing services,
denial and an uncertain delayed reply. Entry fixtures cover service-only manifests
and explicit executable fallbacks. These checks do not claim acceptance of every
installed application's activation behavior in a graphical session.
