# Temporary 4K resource test

This is a prepared test helper, not a native test result. It changes only the
existing Lucent test VM's runtime `Virtual-1` output from 1920×1080/60 Hz/1× to
3840×2160/60 Hz/1.5×. It performs no measurement, installs nothing and writes no
desktop preferences or compositor configuration. Production desktop code and the
archived 26d release remain unchanged.

The host controller is `vm/test-runtime-output.py`. Its default invocation and
imports are inert. `--mode presentation` fails before transport: autonomous
restoration of an enabled tracing transaction is not implemented here. Do not
enable tracing while this resource-only lease is running.

## Ownership and restoration

Before any output mutation, a new private guest directory holds exact source
hashes, the reviewed release/runtime binding and an independent systemd user
guardian. The binding must identify the actual boot, desktop PID, start time,
binary and source cohort, observed within 300 seconds. Native compositor birth,
socket, monitor ID, Wayland global and desktop host generation remain pinned.
The original advertised 4K60 mode, one 1080p/1× output, idle unlocked desktop,
existing stay-awake marker and exact original 4K60 H.264 source are required.
The helper never changes idle or lock policy.

The guardian has a fixed 900-second lease with 120 seconds reserved for cleanup.
Requests cannot extend it. It restores after expiry, controller disconnection or
an operation failure. A systemd lifetime bound provides an additional process
limit. Process/machine destruction or blocked kernel IO cannot promise successful
rollback; the durable final receipt records failure rather than inventing one.
The initial frozen proof uses its existing 50-second fingerprint command bound;
subsequent full media fingerprints have an explicit 45-second command bound.
Native compositor commands retain the canonical six-second bound.

One timer handler is installed before native mutation. It raises only inside a
protected operation, against that operation's absolute deadline. Cleanup selects
its new deadline before rearming; a pending old expiry cannot interrupt that
transition. Timer setup/cancellation faults still attempt the one owned cleanup
and write a failed lifecycle receipt, keeping any verified output restoration
separate. No native restore is retried. The private standalone guardian retains
inactive handlers until it exits so a stale pending alarm cannot skip its final
receipt. A current cleanup deadline still interrupts incomplete cleanup.

Only two literal `hl.monitor` commands exist: the fixed profile and the captured
baseline. The guardian records intent before its one apply attempt. If that
reply is lost, cleanup observes the result and never repeats apply. Restoration
requires the same compositor, output/global/host, supervised runtime, source,
configuration, media and security authority, with tracing OFF. An unexpected
native layout or replaced file is preserved and fails cleanup. No preexisting
output is removed, no replacement output is adopted, and no user window is closed.

Cursor and workspace changes do not grant input authority. Cleanup never warps,
focuses or replays input; it reports whether their observations match the initial
values. Changing resolution can itself cause compositor constraints on cursor or
window placement. The helper does not claim to prevent those effects or detect
all transient activity between observations. User activity invalidates an idle
measurement even if output restoration succeeds.

## Reviewed invocation

Root must first review the frozen helper bytes and create a **new** private
1080p resource binding from actual current observations. The old `d620…` binding
and previous receipts are historical and must not be refreshed or relabelled.

```sh
python3 -I -B vm/test-runtime-output.py
python3 -I -B vm/test-runtime-output.py --begin \
  --binding /absolute/path/to/new-reviewed-1080p-binding.json \
  --binding-sha256 REVIEWED_SHA256 --vm-uuid ACTUAL_VM_UUID
```

The begin receipt records the private guardian directory. Begin starts the
guardian and leaves the original mode active. Pass that receipt to `--apply`;
the separate apply receipt records the observed result. Keep it even if the
reply failed. Never retry a mutation after a lost reply.

```sh
python3 -I -B vm/test-runtime-output.py --apply /absolute/path/to/begin-receipt.json
python3 -I -B vm/test-runtime-output.py --status /absolute/path/to/begin-receipt.json
python3 -I -B vm/test-runtime-output.py --facts /absolute/path/to/begin-receipt.json
python3 -I -B vm/test-runtime-output.py --finish /absolute/path/to/begin-receipt.json
```

Status is read-only file/unit observation, including during cleanup. Facts are
a fresh read-only source/runtime/profile proof, not a resource or FPS sample.
Finish waits for the durable final cleanup receipt; a finishing acknowledgement
alone is insufficient. Neither finish nor status requires refreshing the original
binding after its admission timestamp ages. They still require its unchanged bytes,
controller/source identity, VM UUID and exact independent guardian authority.

## Sampling the profile

After successful apply, facts must prove physical output 3840×2160/60 Hz/1.5×,
logical background 2560×1440, density 1.5, physical background buffer 3840×2160
and actual producer target/frame 3840×2160. The surface's integer scale can be 2;
it is independent of fractional density. A 4K source alone establishes none of
these output or prepared-frame dimensions.

Use a new `4k60-1p5x` resource binding with the actual current boot/PID/start,
unchanged release/media/source pins and a new cohort ID. The existing frozen
82aa wrapper already accepts that profile. Its 30-second warm-up and 120-second
normal-OFF interval remain unchanged, as do the host swap/PSI eligibility gates
and 300 MiB target. Keep enough time for its before/after probes and this owner's
cleanup reserve. Do not start a phase whose bounded budget exceeds the remaining
lease. Archive normal-OFF RSS independently from any future tracing phase.

Finally finish the owner, inspect its restoration receipt and capture fresh
normal-OFF facts at the original output. Record native execution, measurement
eligibility and performance acceptance separately. This helper is not a
Display-panel flow, physical-monitor test, presentation measurement or 60 FPS proof.
