# Guest suspend/resume preparation

This is a source-only plan and read-only prerequisite inspector. **No guest S3,
suspend/resume, secure resume or physical hardware acceptance is established.**
The inspector has no suspend, wakeup, authentication or domain-edit operation.

The approved hardware candidate inspected during preparation is q35 with ACPI,
but explicitly contains `<suspend-to-mem enabled="no"/>`. Libvirt documents that
this disables firmware advertisement of ACPI S3. The actual guest power states,
selected memory mode and logind authorization have not been queried for this
plan. The current domain therefore lacks an affirmative S3 prerequisite. A
paused libvirt domain, process SIGSTOP, or guest `freeze`/`s2idle` is not guest S3.
Enabling S3 would be a separate reviewed domain change/reboot with its existing
conditional XML rollback, not something this helper performs automatically.

## Read-only prerequisites

After review, the VM owner may transfer `vm/inspect-native-suspend.py` and
`vm/native_suspend_policy.py` together, hash-check them, and run the inspector's
explicit `--guest` mode through the existing session transport. It reads only
`/sys/power/state`, `/sys/power/mem_sleep`, boot/clocks and logind's `CanSuspend`
and `ListInhibitors`. Inhibitor names, reasons and environment are omitted.
It refuses a non-QEMU/KVM guest before other observation. It writes nothing.

An offline comparison accepts explicit captured artifacts:

```sh
python3 vm/inspect-native-suspend.py --assess \
  --domain-xml /private/captured-domain.xml \
  --guest-json /private/captured-suspend-prerequisites.json
```

This checks firmware advertisement, `mem`, available and selected `deep`,
regular-user `CanSuspend=yes` and absence of sleep-block inhibitors. Existing
delay inhibitors remain intact. Even a candidate result is **not execution
authority**: effective `systemd/sleep.conf` and its drop-ins must be reviewed so
`systemctl suspend` actually selects `mem`/`deep`; `MemorySleepMode=` may override
the kernel selection. No sysfs writes, config changes, ignored inhibitors or
root authorization bypass are permitted by this plan.

## Bounded execution proposal, after prerequisites pass

1. Bind the final immutable release archive and all installed/running binary and
   support-script hashes, the exact domain UUID/live XML/QEMU PID birth and the
   existing SSH host-key identity. Capture boot ID, supervisor/desktop/compositor
   invocation/PID birth/cgroup/socket authority, current output layout and host
   instances, settings/config inode and content fingerprints, exports, history,
   media source hash/settings and the existing idle marker. Use the established
   read-only preservation helpers; do not replace their comparison rules.
2. Begin with an **already securely locked** session. Observe the compositor's
   acknowledged session-lock state and the exact native lock service/process
   release identity before sleep. Do not enter a password or unlock from the
   helper. `lucent-idle.py` already keeps the normal `swayidle -w before-sleep`
   lock hook even when stay-awake disables its idle timeout; retain that delay
   inhibitor and the existing `lucent-lock.py` fallback/deadline behavior.
3. Arm one independent, bounded host wake owner before requesting sleep. It must
   pin the admitted domain UUID/QEMU process birth and live XML on every query,
   observe QMP `query-status`, and send **only** QMP `system_wakeup` to that same
   guest when its state is `suspended`. Capture QMP `SUSPEND`/`WAKEUP` events
   with a finite event subscription (libvirt's `qemu-monitor-event --loop
   --timeout` supports this). Do not use `virsh suspend/resume`, `cont`, reset,
   shutdown or a guest-agent hibernation path as a substitute. An owner-start
   lost reply must not trigger a second suspend request.
4. Through the normal guest user/session, issue `systemctl --no-ask-password
   suspend` **once**, retaining its exact job/start receipt. Authorization denial
   is blocked evidence; do not force, ignore inhibitors, display authentication
   UI or retry a possibly delivered request. The proposed owner waits at most
   15 seconds for the actual S3 state, wakes it after a 5-second observed sleep,
   and has a 45-second overall bound with an independently retained receipt. If
   its captured domain authority changes, it refuses all power operations.
5. On resume, require the same boot ID, QMP SUSPEND→WAKEUP with an observed
   `suspended` state, kernel journal `PM: suspend entry (deep)` and matching exit
   from the same bounded boot/cursor interval, and a guest BOOTTIME-minus-
   MONOTONIC increase of at least one second. These independent observations
   distinguish guest S3 from host pause. Missing journal/event access is missing
   proof, not permission to infer sleep from a command exit code.
6. Require the compositor still securely locked and all admitted production
   processes/units/socket lifetimes, outputs/settings/config/media/exports/history
   exact. The video may resume at a later PTS; its selected source and playback
   policy must not change. Lock may intentionally freeze desktop video: require
   secure lock rendering after resume, then verify normal video/frame advancement
   after a separate operator unlock. No unlock/UI input is injected by this
   helper. Preserve preexisting lock/idle/session state and retain failed
   receipts without a global rollback, service restart or foreign-state rewrite.

The independent wake owner and mutating executor are deliberately **not yet
implemented or approved**: the captured domain currently disables S3 and the
actual guest/effective sleep policy/lock prerequisite evidence is absent. The
finite bounds above are a proposed test budget, not measured latency. No RAM,
FPS, power saving, cable hotplug or physical GPU suspend claim follows from it.

## Source references and checks

Primary local documentation reviewed: libvirt `formatdomain.html` “Power
Management” (firmware S3/S4 advertisement), `virsh(1)` “qemu-monitor-event” and
“dompmsuspend/dompmwakeup” (events versus guest PM, and guest-agent requirements),
and `systemd-sleep.conf(5)` `MemorySleepMode=` (effective sysfs selection).
Repository references are `vm/lab.py`, `scripts/lucent-idle.py` and
`scripts/lucent-lock.py`. No current external documentation was fetched.

`tests/test_native_suspend.py` exercises disabled firmware, s2idle/freeze,
authorization/inhibitor refusal, private inhibitor redaction, non-VM refusal and
S3-versus-host-pause/reboot/clock-gap evidence. These are pure/mocked checks;
they do not invoke a power operation, contact a VM or establish native acceptance.
