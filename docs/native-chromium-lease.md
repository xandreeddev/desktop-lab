# Test-owned Chromium execution lease

`vm/owned_chromium_lease.py` belongs to native acceptance tooling. It does not
change the SDK, desktop runtime, application adapter or generic unit checks.

Normal Chromium can retain its original wrapper PID and birth while moving from
the launching transient service into a registered application scope. The service
can still report that MainPID and invocation. Requiring its process to remain in
the original cgroup correctly fails a generic unit check; a browser-specific
lease supplies the additional migration proof instead of relaxing that check.

The wrapper's durable receipt identifies the original service invocation,
MainPID and process birth before exec. Initialization pins the installed
root-owned launcher and actual Chromium executable, with bounded streaming
hashes up to 512 MiB, plus the explicit unique profile/class and declared startup
arguments. Admission requires that exact original process to exec that binary.
If its kernel cgroup changed, the helper queries the already registered scope's
typed ID, invocation, active state and ControlGroup. It freezes that scope and
rejects later movement or successor invocations. It never creates, guesses or
stops an application scope.

Chromium may flatten its process title into one argument. The helper preserves
those bounded opaque bytes by digest. It checks unambiguous literal profile,
class and fixed startup flag boundaries, without splitting arbitrary application
text or interpreting shell syntax. Per-input admission retains the exact process
birth, installed executable identity, declared startup and frozen scope. The App
and GUI fixtures separately freeze their own DevTools endpoint and original
control target before subsequent CDP operations.

Retirement opens a PIDfd, revalidates the complete lease, and sends graceful
SIGTERM only through that descriptor. It observes the original process exit and
both original service and scope lifetimes, and verifies their recursive cgroups
are empty or gone. It sends no process-name signal, guessed-PID signal, scope stop
or broad descendant kill. A replacement invocation or incomplete drain fails and
preserves the private profile and receipts.

The App fixture reuses its existing independent watchdog and durable first-browser
lease; forwarding wrapper services retain their strict generic origin checks.
The GUI wrapper starts a separate watchdog before exec, records its own original
unit receipt, and requires a durable browser admission before host input. That
watchdog requests lease retirement after 180 seconds; its own unit and process
have independent 240/235-second limits. These limits bound the helper; they are
not substitutes for a successful process/scope drain observation. Notes and
clipboard restoration still require the normal guarded live-host path.

The migration and flattened-title observations explain a failed native harness
attempt. The corrected lease and integrations have source/pure-test evidence;
their native App and GUI reruns remain pending. This is not a production launch
failure, a third-party compatibility pass or a newly measured performance result.
Private receipts include controlled test metadata only; public evidence must omit
process identities, temporary profile paths, protocol endpoints and text payloads.
