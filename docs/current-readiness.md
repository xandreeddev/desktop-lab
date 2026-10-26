# Current daily-use readiness

The named accepted-main and verified delivery checkpoint is
[`912074f1`](https://github.com/xandreeddev/desktop-lab/commit/912074f1678b67b789b75217ea253d40dd327e73),
through [PR115](https://github.com/xandreeddev/desktop-lab/pull/115).
[Pages40414422493](https://github.com/xandreeddev/desktop-lab/actions/runs/40414422493)
passes three exact public HTML artifact matches and the native manifest/PNG:
**118 producer inputs and 480 captures from source 3640bd86** (public receipt
`695a4c42b0f018eaf0339d947bf2814c8c1440deb1437bea55a22c133e8e23ea`).
Twenty same-artifact LAN HTTP checks pass, retaining the 64/128 MiB static-server
limits (receipt `7f47d1a2d4c22760cdc8b337d65d581a2e4d424387dd73b659493042067833db`).
Earlier 9d42/PR114/Pages40396627106 served 4a0f932a's **117** producers;
ca88/Pages40392718403 served b3c91fab's **116**. Those checkpoints remain historical.
This later focus-investigation, CI and delivery reconciliation still needs its own
final build, protected-main merge and publication. Delivery establishes no native
keyboard fix or performance acceptance.

Archived resource and earlier feature observations below retain `eb3481f4` and
their other named sources. At the earlier [PR110](https://github.com/xandreeddev/desktop-lab/pull/110)
checkpoint, main `9b52666b` matched that archive's Rust/design/configuration/scripts
trees. The archive was built at eb3481f4; its original capture-time flags remain
unchanged. Later commits and delivery do not rebuild or rerun those measurements.
Earlier checkpoints remain in [the ledger](implementation-status.json).

The core Wi-Fi repair, safe display editing, international text and explicit
application-opening paths are implemented with scoped native evidence. The
seven-gap goal is **not complete**: cold keyboard handoff, eligible ≤300 MiB and
source-rate presentation acceptance, physical devices and broader application
interoperability remain open. Delivery and a green test suite do not establish
those results.

Candidate `3640bd86c6ec` implements the typed
[`Effects::after_input_release`](lucent-framework.md) handshake and passive-toast
keyboard policy described in [default-handler activation](default-uri-activation.md).
Its coherent build completes **17 stages and 823 targeted test passes**. A separate
actual AMD RADV offscreen export covers **118 producers, nine stories, five
palettes and 480 PNGs**, all byte-identical to the prior RADV reference. The
candidate is now coherently deployed: all six user clients and the root greeter
are verified, with user/settings/security state preserved and no greetd restart.
The actual [cold-browser source 3640 run](../reports/measurements/lucent-cold-default-uri-3640.json)
passes pointer opening and one trusted `KeyA`; the separate Tab/Enter route still
fails browser focus/key delivery. Both owned cleanups pass all nine checks. The
last pre-key history/input-release/frame-advance guards pass, but the absent
keyboard event count and post-event observation remain `null`. The failure cause
and a keyboard fix remain unestablished. The earlier
[eb348 cold run](../reports/measurements/lucent-cold-default-uri-eb348.json) keeps
its original outcome. This source 3640 deployment was an unmerged candidate at capture;
the archive's original capture flags are unchanged.

The later [focus investigation](../reports/measurements/lucent-cold-default-uri-focus-diagnostic-3640.json)
on unchanged 3640 repeats canonical pointer success and keyboard failure. Moving
only the cursor to the pointer route's link point while retaining Tab/Enter makes
a separate diagnostic receive document focus and one trusted `KeyA`; canonical
acceptance is explicitly false. All three cases pass their own nine cleanup
checks. The comparison supports an under-cursor widget hypothesis, but the
non-atomic geometry/requested-policy recorder exposes no actual seat owner and
establishes no cause or production fix. Source `0a981631dca2`/`b2f54551dfbe` prepares
member-aware widget suppression and read-only native keyboard diagnostics. Exact
b2 passes the Ubuntu [Rust job](https://github.com/xandreeddev/desktop-lab/actions/runs/40411820911/job/121884163923)
in [CI run40411820911](https://github.com/xandreeddev/desktop-lab/actions/runs/40411820911):
workspace/protocol/software-Vulkan suites and all 17 individually verified new
tests pass. A coherent Arch release, candidate VM deployment and native keyboard
acceptance remain pending. See
[the activation guide](default-uri-activation.md#focus-geometry-and-cursor-only-diagnostic)
for the requested-versus-committed ordering limitation. The seven-gap goal stays open.

The 118 source3640 capture inputs are now published at main912, separate from
historical 9d42's 117 inputs and ca88/b3's 116 inputs. This does not deploy the b2
widget policy or diagnostic runtime to the VM.

The recent changes add:

- [PR102](https://github.com/xandreeddev/desktop-lab/pull/102): reusable decoder
  sessions, bounded stream uploads and default-handler activation context.
  `UriPort` now carries `ApplicationLaunchContext`; notification links request
  activation without inventing the unknown handler's app ID, then revalidate
  the notification/link before the adapter opens it. The later R9 run passes
  scoped warm-browser default-route transport through native pointer and
  Tab/Enter input. The later `9eb41939` run below adds natural history release
  and actual document key delivery; broader handler behavior remains separate.
- [PR98](https://github.com/xandreeddev/desktop-lab/pull/98): bar handoff and panel
  lifecycle changes on main `726776de`. A separate `8a3cd7d8` candidate passes
  nine native panel/input/reservation checks and strict headless Vulkan pixel
  comparisons. Those observations do not prove transient no-flash behavior or
  animation pacing, and do not accept the candidate's renderer/performance work.
- [PR99](https://github.com/xandreeddev/desktop-lab/pull/99): the opt-in
  [development reload tool](dev-reload.md), on main `6ee86830`. Its VM trial
  applies a valid bundle, preserves an open terminal, rejects a damaged manifest
  without restarting, and rolls back a deliberately failing candidate. The
  tested bundle belongs to `8a3cd7d8`; combined save/build/watch operation on an
  installed physical desktop remains separate acceptance.
- [PR100](https://github.com/xandreeddev/desktop-lab/pull/100): the
  [component lab](component-lab.md), with nine stories and 480 actual Vulkan
  captures from `b9de7ba4`. Eight native interaction and four restoration checks
  pass on the independent Lab client. They cover story selection, hover/focus,
  edit events, slider/toggle events, popover opener/Escape focus, Menu keyboard
  ownership and animation submissions settling. They establish neither generic
  outside-click/focus-trap behavior nor clipboard/IME interoperability, numeric
  fixture values, FPS or the entire desktop's performance.

These are recorded source-specific observations, not a live VM status. The
previous accepted-main/PR100 delivery on 2026-10-22 at `124b2069` is preserved
as a historical snapshot in [the ledger](implementation-status.json). The
accepted `8e6d074b` runtime evidence, later unmerged `35ed87ed` resource trial and
`8a3cd7d8` candidate workflows retain separate identities. No candidate is
promoted to accepted main by a documentation edit. Earlier delivery receipts,
failed attempts and their independent recoveries remain in
[the ledger](implementation-status.json) and the source-specific reports below.
Accessibility remains user-deferred.

## Latest same-runtime memory sequence

The [eb348 output/resource matrix](../reports/measurements/lucent-output-resource-matrix-eb348.json)
records a successful 15-stage build with 786 targeted tests and coherent six-client/
root-greeter deployment. Three acceptance executables are staged; staging is not
native feature testing. User/security state and supervision are verified without
restarting greetd or writing settings/themes.

Three uninstrumented intervals use the same boot, PID, process birth, binary,
media, settings and adapter. Each runs 120 seconds with 480 RSS samples and zero
process swap. Playback is configured to source rate for the same 3840×2160 H.264
60 FPS source; no presentation FPS is collected.

| Recorded output phase | Mean / sampled peak RSS | CPU, % of one logical core |
| --- | ---: | ---: |
| Cold FHD, 1920×1080/60 Hz/1× | 229.181 / 230.875 MiB | 142.749% |
| 4K, 3840×2160/60 Hz/1.5× | 494.683 / 526.516 MiB | 191.599% |
| Restored warm FHD | 435.063 / 435.063 MiB | 146.541% |

All three intervals are **host-pressure-ineligible**; memory-target results remain
null. The owned 4K finish verifies original FHD, source/configuration/security,
cursor/workspace and runtime preservation, with no restart, input or window close.
The warm interval then confirms that elevated memory remains in the same process.

Current-frame bytes return from 33,177,600 at 4K to the cold-FHD value 8,294,400;
prepared-image bytes return from 48,037,279 to 14,859,679. Active GPU-cache payload
returns near its cold value, while warm smaps Anonymous remains 372,212 KiB versus
cold 161,408 KiB. These counters overlap and do not form an additive RSS ledger.
A separate final procfs observation adds VMA-label groups, not allocation owners.
Unused pool/codec/staging allocations and allocator/driver backing need isolated
measurement. No cause, leak, CPU-reuse gain, FPS or eligible budget is established.
The 578.984 MiB lifetime HWM is not the warm interval's 435.063 MiB sampled peak.

A separate instrumented FHD follow-up on the same source records **59.842 distinct
prepared-revision advances/s**: 7,180 advances over 119.983328 compositor seconds,
from 7,181 background feedbacks. Gap p50/p95/p99 is 16.667 ms; maximum is 33.334 ms.
The six-group aggregate is not the background numerator. No sampling truncation is
reported, but two background feedback requests remain pending; complete feedback,
visible pixels, physical scanout and launcher-animation acceptance are not established.

That follow-up has a different process birth: its 236.039/236.789 MiB mean/peak
RSS and 146.841% CPU do not resolve the warm-memory concern. The 120.000521-second
interval and its normal reference remain host-pressure-ineligible, with unmatched
I/O pressure and null targets. Separate driver/restore receipts verify original
source/security/settings and return to normal profiling OFF. This adds no eligible
60 FPS or causal CPU-reuse gain and leaves the original memory matrix unchanged.

## Read-only host service observations

The [host observer report](../reports/measurements/lucent-host-service-observations-6f9.json)
records four separately built consumers from `6f9e75d0`, each observing the real host
user's services for 15 seconds without controls or input. Four mock tests, strict release
Clippy and all four binary builds pass. Every observed process exits successfully and
its owned unit is inactive/collected; the audio watch explicitly returns after its own
cancellation.

Audio reports **3 outputs / 5 inputs**, including **3 monitor inputs**. Bluetooth
reports **1 adapter / 3 catalog devices / 1 connected / 0 pairing requests**. Power's
count-only snapshot reports **battery_present=false / 0 profiles / 0 backlights**, but conflates
absent and unavailable capabilities. Compositor observation reports **1 monitor /
5 workspaces / 6 windows**, with unambiguous focus. These establish adapter observation,
not audible playback/recording, a particular paired keyboard or key events, pairing,
physical controls, changed-state delivery, hotplug, suspend, desktop UI or performance.
All broader hardware and seven-gap acceptance limits remain open.

## Recorded 9eb matched-source FHD video measurements

The [completed V2 report](../reports/measurements/lucent-fhd-timing-9eb-v2.json)
records two 120-second phases on archived 9eb41939, one virtual 1920×1080/60 Hz/1×
output and RADV/Vulkan. Both use the same 3840×2160 H.264 60 FPS source
with playback configured to source rate; they have different process lifetimes.

With all tracing OFF, 480 normal samples record **497.063 MiB mean/peak RSS**,
**616.008 MiB process-lifetime HWM**, **142.633% of one logical core** and zero
process swap over 120.000454 seconds. Memory investigation remains necessary.
Host 9,634 swap-in pages and 0.423044 I/O-full stalls make this interval ineligible;
the 300 MiB target remains null.

The separate instrumented phase runs 120.000898 seconds. Its nominal request
window has 7,180 background feedbacks referencing 7,180 distinct prepared revisions:
7,179 advances over 119.983329 compositor seconds, or **59.833 advances/s**. Gap
p50/p95/p99 is 16.667 ms and maximum 50 ms. This is bound-resource feedback,
not a visible-pixel or physical-scanout measurement. Host 32,833 swap-in pages,
0.401464 I/O-full stalls and unmatched pressure make this phase ineligible too.
Its 231.785/236.441 MiB mean/peak RSS belongs to a different process birth and
establishes no memory improvement. Neither interval tests the newer PR108 CPU pool.

Both collectors verify preservation. Driver restoration and the later independent
guardian-terminal/normal-runtime receipt verify profiling OFF and the original
binary; no restart or hardware behavior is inferred beyond those records. The
historical V1 coverage failure remains failed and unattributed. Source-rate
presentation and the memory target remain unaccepted; eligible measurements,
physical devices and broader application compatibility remain open.

## Recorded 9eb deployment and keyboard handoff

The [source-specific trial report](../reports/measurements/lucent-runtime-9eb-native-trial.json)
records a terminal build of `9eb41939d47ced5be2012bff41a1f2f5768d6ee7`, manifest
`c669b9d5`, with 15 successful stages and 761 targeted test passes. Its coherent
six-client/root-greeter deployment preserves user/security state, survives the startup
deadline and stages three acceptance executables without restarting greetd or writing
settings/themes. The archive's original UNMERGED flags remain intact as capture-time
history. PR106 later accepts the combined runtime code through the independently
verified source equality above; the build and native run still belong to `9eb41939`.

Harness `04432a73` on that exact runtime passes five default-route checks, nine cleanup
checks and **both native browser key-delivery cases**. A real pointer link and Tab/Enter
link each open one selected document through a controlled temporary MIME default in an
already warm real Chromium profile. After natural history close, the exact document
receives one trusted, unmodified, nonrepeating `a` / `KeyA` guest event. The final pre-key
and post-event guards both observe closed history, an advanced native submission and
normal-toast input policy. No forced focus, forced history hide or key retry substitutes
for this evidence. This establishes scoped keyboard delivery beyond an active-window
match. Fresh-browser initial focus, arbitrary handlers, external navigation and direct
Chromium token consumption remain unverified. The harness source passes 774 Python
checks, including 87 focused default-URI cases.

A separate uninstrumented **120.000484-second**, 480-sample FHD interval records
**230.063 MiB mean / 230.129 MiB sampled peak RSS**, **239.676 MiB process-lifetime
high-water RSS**, **147.558% of one logical core** and zero process swap. It uses one
1920×1080/60 Hz/scale-1 virtual output with the fixed 3840×2160 H.264 60 FPS source
configured for source-rate playback; that source resolution is not a 4K output test.
Preservation passes and frame/presentation/source tracing remains OFF. Host **15 swap-in
pages** and **0.450095 I/O full-stall fraction** make the interval **ineligible**; the
300 MiB target result remains `null`. No FPS samples or causal improvement over earlier
238 MiB intervals or the one-off 605 MiB diagnostic follows. The later
[9eb 4K follow-up](../reports/measurements/lucent-4k-setup-and-memory-followup-9eb.json)
verifies actual 3840×2160/60 Hz/scale-1.5 setup, a 2560×1440 logical background and
4K producer frames, then restores the original FHD output with configuration/security/
runtime preservation. Its sampler refuses host-memory admission before facts, resource
binding or interval collection. Independent restored-FHD/all-OFF observation records a
one-off **490.176 MiB RSS**, **422.914 MiB anonymous RSS**, **616.008 MiB process-lifetime
HWM** and zero swap. This warrants memory investigation; it is not a steady-state
interval, 4K peak, causal comparison or accepted target.

A separate resource-only FHD timing V1 attempt fails
`inspection_gap_prevents_interval_coverage`, with no completed 120-second interval or
profiling enable. The later completed V2 intervals above do not establish the actual
V1 failure cause. A separate 30-second
read-only host I/O diagnostic is complete: global I/O full stalls are 7.13%, CPU busy
is 87.85%, and 169 swap-in pages are observed. It finds no sustained NVMe saturation
evidence and leaves the cause unresolved, with no code change, headroom-recovery proof
or performance acceptance. This host observation is separate from the FHD guest interval.
The earlier source checkpoint passed 774 Python cases and the catalogue check.
PR107 delivery remains historical; the later PR108 delivery checkpoint is recorded
above. Performance, physical hardware and the overall seven-gap goal remain open; accessibility remains user-deferred.

## Recorded 26d observations captured before merge

The [PR102](https://github.com/xandreeddev/desktop-lab/pull/102)
trial used a coherent release built at
`26d03adf476af44c97fb7dcdbfb75cbdc2112770`. Its later documentation integration
head is `3560e3b6`; neither that head nor the later merge changes the archived
binary's source identity. The report was captured while PR102 was unmerged and
retains that historical flag. Its code is now included in accepted main as above.
The [source-specific native report](../reports/measurements/lucent-runtime-26d-native-trial.json)
records a successful six-client/root-greeter deployment with user state and
security preserved. An owned-peer IME run on the same source passes five feature
and five restoration checks, including visible preedit ink and caret advance.
Third-party IMEs, physical/cross-output input and broader clipboard workflows
remain separate acceptance.

A normal, uninstrumented **120.000801-second** interval on that candidate
observes **237.983 MiB mean / 238.035 MiB sampled peak RSS**, **240.215 MiB
process-lifetime high-water RSS**, **194.290% of one logical core** and zero
desktop-process swap. The actual output is one virtual **1920×1080 at 60 Hz,
scale 1**, using RADV; the unchanged **3840×2160 H.264 60 FPS source** is
configured for source-rate playback. The source video resolution is separate from the output resolution.
Density, effects and motion were preserved.

The interval is **ineligible**: the host records **6094 swap-in pages** and
**0.350258 I/O full-stall fraction**. The normal RSS-target result remains
`null`, despite the observed sampled peak below 300 MiB. CPU, presentation and
source tracing are off; no presentation/PTS samples exist. This supplies no FPS,
animation-pacing, causal performance-gain or actual 4K/scale-1.5 output result.

A separate Display attempt records five checks before an overall failure,
including the four earlier owned-peer flows and a peer-rotation Apply/Revert
observation. It fails with `bounded_guest_transport_failed`; the owner reports
`success=false`, `restoration=null` and
`foreign_or_uncertain_cursor_movement_blocks_warp`. Those observations establish
no successful full trial, completed restoration, native enable/disable or input
on a fractionally scaled Displays panel. Any later recovery requires separate
evidence; it cannot change this failed report's outcome.

A subsequent, separate guarded recovery completes with exit 0 and empty stderr.
It verifies the original monitor bytes/inode, removes only the owned output,
asserts the original cursor/workspace and preserves the original sources,
security, settings and media. It grants no input or cursor-warp authority;
monitor ctime and parent timestamps are outside its preservation claim. The
[candidate report](../reports/measurements/lucent-runtime-26d-native-trial.json)
also records an independent read-only observation afterward: the same candidate
source and desktop digest, one restored 1080p/scale-1 output and all three
profiling paths off. This recovery supplies no new Display feature-flow pass,
presentation or resource acceptance. The original failed attempt retains its
`success=false` and `restoration=null` flags.

These receipts supplement the historical snapshots above. The separate merge and
publication receipts establish delivery; they do not close the performance or hardware
gaps or mark the seven-gap goal complete. The initial default-handler harness
preparation at `cea7b8e6` recorded 27 focused pure cases and 631 passing integration
checks at that source. That initial checkpoint established no native acceptance. The
preserved initial warm-browser setup attempt failed before rich-link input, with no
feature checks or restoration established by that attempt; a separate recovery cannot
rewrite its outcome.

A later V7 harness at `de276755` ran natively: the temporary MIME handler was selected,
all nine post-warm steps completed, and native pointer focus on the notification group
passed. It failed at `pointer_target_revalidate` /
`one_current_selected_notification_link` before dispatching the link press. The timing
diagnostic records the observed revision at 16.681 seconds, fixture expiry at 24.999
seconds and finish request at 27.064 seconds; the controlled 25-second lifetime expired
during the intervening audit guards. All nine cleanup checks pass. A separate later
read-only normal-OFF observation confirms the same desktop lifetime, original output and
unchanged 14 source/43 security entries with all tracing OFF. This is a failed pre-link
attempt, not default-handler activation, token-delivery or browser-interaction
acceptance. R8 later reached pointer dispatch but failed default-handler delivery: the
installed generic `xdg-open` path retained quotes around the fixture’s fixed `-I`
argument. Its failure and native-input counts remain unchanged; the fixture-only R9
correction is covered below.

## Later 4K preflights and one-off memory diagnostic

The [later 4K preflight
follow-up](../reports/measurements/lucent-4k-resource-preflight-followup-26d.json)
records two successful owned output setups and restorations on runtime `26d03adf`:
physical buffers of 3840×2160 at 60 Hz/scale 1.5, with a 2560×1440 logical background
and 4K producer frames. Neither run collected a resource interval. Sampler A refused a
remaining lease below the unchanged 520-second measurement/cleanup budget; B refused
host-memory admission before sampler facts or measurement-binding creation. Both
restored the original FHD output, configuration, security/runtime identity, cursor and
workspace. A separate later read-only normal-OFF observation verifies the same desktop
lifetime, 14 source/43 security entries and all tracing OFF.

A separate one-off proc diagnostic after B restoration, at the original 1920×1080/60
Hz/1× configuration, records **604.594 MiB RSS**, **539.586 MiB anonymous RSS**,
**16.242 MiB process swap** and **680.293 MiB process-lifetime high-water RSS**. These
are one observation and a cumulative high-water mark, not a 120-second eligible sample
or the peak of the 4K phase.

The diagnostic motivates a bounded memory investigation. It does not attribute memory to
4K, a particular allocator/decoder/renderer, GPU allocations or a leak. The earlier
237.983/238.035 MiB interval keeps its original source, workload and failed
host-pressure eligibility. The 300 MiB target, source-rate presentation and animation
pacing remain unaccepted.

## Later warm default-handler route acceptance

The [R9 default-handler
report](../reports/measurements/lucent-default-uri-native-26d.json), harness `563781a2`
on runtime `26d03adf`, passes all five scoped checks and all nine cleanup checks. A real
pointer link activation and keyboard Tab/Enter activation, using two Tab events, each
deliver fresh input-bound context through the controlled temporary MIME default and open
exactly one selected document in an already warm real Chromium profile. The other checks
cover handler selection and notification expiry. Token contents are not published.

Both Hyprland active-window observations match the owned browser. This is not evidence
of seat keyboard focus or a browser receiving a key. The harness explicitly hides
notification history between cases and during cleanup; it does not prove automatic
history release, natural browser keyboard handoff, fresh-browser initial focus or direct
Chromium token consumption. Those user-interaction boundaries remain open, along with
arbitrary handlers and external navigation.

The separate client fix `194c0c57` implements current-session-only history release after
successful URI handoff. Its exact three-file source passes 372 desktop tests, with six
ignored, and release Clippy with warnings denied; all ten new handoff cases pass. It is
not deployed in the recorded `26d03adf` VM. The later `9eb41939` report above
separately establishes deployment and scoped warm-browser key delivery; it does not
change this older route-only report.

All nine cleanup checks pass. A separate later read-only normal-OFF observation verifies
the same desktop lifetime, original 1920×1080/60 Hz/1× output and unchanged 14 source/43
security entries, with frame, presentation and source-PTS tracing OFF. R9 changes only
the fixture’s strictly allowlisted fixed arguments; the canonical general desktop-entry
encoder stays unchanged. Earlier failed attempts, including R8’s actual pointer
dispatch, keep their original outcomes. No resource, pacing, hardware or overall-goal
acceptance follows.

## Later nine-flow virtual Display acceptance

The [later nine-flow Display
report](../reports/measurements/lucent-native-two-output-displays-26d-nine.json) records
a native VM run on runtime `26d03adf`, using harness `1774fa87`. It passes all nine
Display workflows: draft-only edits, Apply/Revert, independent scale timeout, position
Keep, rotation/Revert, disable/Keep, re-enable through the actual controls/Keep,
fractional scale/Keep, and pointer/keyboard editing with Apply/Revert on the fractional
peer. That panel rendered at 1024×768 logical units, density 1.25, with a 1280×960
buffer and 26 recorded native submissions. Submissions establish rendered geometry; they
do not establish presentation FPS.

The owner passes all nine bounded setup/restoration phases, all six restoration fields
(five preservation checks and the timestamp/link-count scope marker) and all three
cursor checks. Original monitor bytes/inode, the original output, workspace, cursor,
settings/media, source/security files, supervision and normal profiling state are
preserved; only the owned peer is removed. Monitor ctime, link count and parent entry
timestamps are not claimed unchanged. A separate later read-only normal-OFF probe also
passes on the same desktop lifetime and restored original 1920×1080/60 Hz/1× output.
Frame profiling, presentation tracing and source-PTS tracing remain OFF, and the same 14
source and 43 security entries match the pre-run inventories. This independent
observation does not rewrite the Display receipt.

This closes the scoped virtual-output rotation, enable/disable and fractional
Displays-panel input gaps for this source cohort. Physical monitors/cables,
suspend/resume, compositor restart, secure-output changes, pacing and eligible
performance targets remain open. Earlier failed attempts remain unchanged historical
evidence. The report makes no claim about the VM's current running state or completion
of the overall goal.

## What the status words mean

| Claim | What it establishes | What it does not establish |
| --- | --- | --- |
| Implemented | The production code path and its stated boundary exist. | Complete behavior, passed checks or daily-use reliability. |
| Automated checks pass | The named source passed its listed pure, protocol, client or pixel checks. Private D-Bus fixtures exercise real transport against a controlled peer. | Native desktop interaction, production service policy or hardware compatibility. |
| Owned native fixture passes | The named binary ran in the compositor and the harness exercised the listed interactions against owned peers, with stated restoration checks. | Arbitrary applications, physical devices, every output configuration or smooth presentation. |
| Real application / hardware accepted | A report identifies the actual application or physical device, workload, binary and observations. | Other applications, drivers, devices or workloads. |
| Merged | The source is present in accepted main. | That every feature has native or hardware acceptance. |

Evidence is specific to a source, binary, workload and test scope. A newer source
does not automatically repeat an earlier native test. Historical reports retain
their original results, limitations and pending items, including items later
resolved. Use this guide and `current_rollup` in
[the ledger](implementation-status.json) for the current reconciliation; the
ledger's requirement history remains useful for the broader backlog.

## The seven requested gaps

| Area | Source state | Still needed |
| --- | --- | --- |
| Saved Wi-Fi recovery | Merged native masked SecretAgent repair retains the saved profile UUID and storage policy. Fourteen native NetworkManager/GUI checks pass against owned simulated WPA2/hidden radios, including cancellation, repair, DHCP, saved reconnection and exact restoration. | Enterprise/WEP provisioning, agent-owned vaults and physical-radio acceptance remain separate gaps; they are not implied by the owned fixture. |
| Safe display settings | Native whole-layout drafts, Apply → Keep/Revert and independent 15-second/UI-crash EOF rollback. Seven earlier one-output flows and the historical four-flow two-output run retain their results. The later [nine-flow run](../reports/measurements/lucent-native-two-output-displays-26d-nine.json), harness `1774fa87` on runtime `26d03adf`, also covers rotation, disable/re-enable, fractional scale Keep and native pointer/keyboard input on a scale-1.25 Displays panel. Five positive preservation checks, one explicit ctime/link-count scope marker, three cursor checks and independent later normal-OFF verification pass. | Physical displays/cable hotplug, suspend/resume, compositor restart, secure-output changes and pacing remain open. Monitor ctime, link count and parent entry timestamps are not claimed unchanged. Earlier failed attempts remain failed; their separate recoveries are not feature passes. The Keep-to-file-identity-capture host-loss interval retains manual recovery. |
| Resources and animation | The new [same-runtime eb348 matrix](../reports/measurements/lucent-output-resource-matrix-eb348.json) completes cold FHD →4K →restored warm FHD with preservation/all tracing OFF. Mean/peak RSS is 229.181/230.875, 494.683/526.516 and 435.063/435.063 MiB respectively; all host-pressure-ineligible. Earlier 9eb/26d/35ed and shape/presentation reports retain their own values and limitations. | Warm FHD memory remains elevated even after current-frame/prepared-image payloads return to FHD values. Isolate pool/codec/staging/allocator/driver ownership; collect eligible resource and presentation evidence. Overlapping payload/RSS counters, same-runtime sequencing and VMA labels do not establish cause, leak, CPU-reuse gain, visible pixels or physical scanout. Preserve earlier failed attempts and independent recoveries. |
| International text | Merged `8e6d074b` implements script shaping, hard-paragraph context across soft wraps, exact row-bound visual source/caret maps, tab separator projection, deterministic fallback and opt-in text-input-v3 composition. Five owned native feature checks pass: visible preedit/caret advance, CJK commit/delete/history, masked-focus retirement, multiline composition and RTL navigation. | The fresh owned native suite passes all five feature and five restoration checks; earlier cleanup failures retain their original results. Third-party IME, broader native pixel/scroll details, delayed clipboard/focus races, physical/cross-output text and broader GUI clipboard coverage remain pending. The later Chromium/Notes test passes both directions with nine interaction and twelve restoration checks on one virtual output. Exact merged `8e6d074b` passes six CI checks, full workspace, six controlled service-protocol filters and 832 Vulkan desktop cases; those checks retain their own scope. |
| Application integration | Accepted main includes fresh input-bound context, explicit file/URI Open, expanded XDG parsing and the canonical default-handler `UriPort::open(target, context)` path. Notification links use the SDK's optional app ID and revalidate their revision/link around token delivery; `XdgUri` forwards that context through the shared application-unit policy. Earlier local-file `8e` and HTTP `%U` `9b` results keep their original scope. The `4c9a4739` harness on coherent `35ed` passes **twelve singular HTTP `%u` feature and nine restoration checks**: pointer, socket and CLI Open each create exactly one fresh owned HTTP document; the declared Action still creates two local documents. The later [R9 default-route run](../reports/measurements/lucent-default-uri-native-26d.json), harness `563781a2` on `26d03adf`, passes five scoped checks and nine cleanup checks: native pointer and Tab/Enter each load one exact selected document through the temporary default handler with fresh context, using an already warm real browser. PR105 accepts source fix `194c0c57`; the later [9eb run](../reports/measurements/lucent-runtime-9eb-native-trial.json) separately passes five route/nine cleanup checks and both trusted browser KeyA cases after natural history close, using the same controlled warm-browser scope. The coherently deployed unmerged [source 3640 trial](../reports/measurements/lucent-cold-default-uri-3640.json) passes the cold pointer route but fails the separate keyboard route; all nine cleanup checks per route pass. | The current cold keyboard route fails browser focus/trusted KeyA delivery despite the last pre-key history/input/frame guards passing. Its missing event and post-event observations remain null, and the cause and a fix are unestablished. Historical warm-browser delivery and the older R9 active-window observation keep their own scopes. Direct protocol-token consumption, external navigation, other handlers/applications and broader workflows remain open. Historical explicit-application and failed default-route reports keep their source/input/cleanup outcomes. Application acceptance does not accept performance. |
| Real-world validation | [Two virtual outputs](../reports/measurements/lucent-native-multi-output.json) pass sixteen native checks on `8e`; [nine Display workflows](../reports/measurements/lucent-native-two-output-displays-26d-nine.json) pass on `26d`, including fractional-panel input. Unchanged `8e` passes [genuine wired-only tray](../reports/measurements/lucent-native-third-party-tray-8e.json) five interaction/four cleanup/thirteen preservation checks, [Chromium–Notes clipboard](../reports/measurements/lucent-native-gui-clipboard-2044.json) nine interaction/twelve restoration checks, and [owned mapped-peer IME](../reports/measurements/lucent-international-text.json) five feature/five restoration checks. Earlier software-HCI Bluetooth and virtual audio control results keep their sources. | Physical monitors/cable hotplug, actual sleep/secure resume, physical keyboard/radio/pairing and audible output/microphone/routing remain open. Additional tray apps/menus, third-party IMEs, other GUI clipboard apps/rich content, delayed-provider/focus races and cross-output text/clipboard need execution. [Suspend inspection](../reports/measurements/lucent-suspend-prerequisites.json) issued no sleep request; [host counts](../reports/measurements/lucent-host-service-observations-6f9.json) sent no input/controls. Historical passes do not accept candidate `3640` or performance. |
| Documentation accuracy | Named accepted main `9d42c20e`/PR114, [Pages40396627106](https://github.com/xandreeddev/desktop-lab/actions/runs/40396627106) and same-artifact LAN receipt `dbd81b26…` verify three HTML pages, native manifest/PNG, **117 inputs/480 captures from `4a0f932a`**, and twenty HTTP checks. Prior ca88/b3/116 delivery remains historical. Active evidence separates virtual outputs, genuine applications, owned peers and physical devices; failed reports keep their flags. | This later framework/text/URI/ledger reconciliation needs its own final build/link/Pages checks and protected-main delivery. Publication supplies no extra native controls, physical interoperability, eligible performance or full-goal acceptance. |

The later [native HTTP URI result](../reports/measurements/lucent-native-http-uris-9b.json)
uses harness `7a92749e` and coherent `9b5420b7` desktop/CLI/consumer. Pointer,
socket and CLI Open each create two fresh exact HTTP documents in a real owned
Chromium profile; the declared Action retains local-file coverage. All eleven
feature and nine restoration checks pass. The direct `%u`/`%U` parser regression
also actually passes exact-source CI. Those historical reports remain unchanged.
A later [singular HTTP result](../reports/measurements/lucent-native-http-singular-uri-35ed.json)
uses the version-controlled `4c9a4739` harness on coherent `35ed`: all twelve
feature and nine restoration checks pass. Each pointer/socket/CLI Open requires
one fresh exact HTTP document; the declared Action independently requires two
local files. Default handlers, external navigation, other applications and direct
activation-token consumption remain unverified. This does not rerun earlier
local-file or plural-URI evidence or accept the trial’s performance.

The relevant contract guides are [network](native-network.md),
[safe display settings](native-displays.md), [output profiles](monitor-layouts.md), [density](fractional-scaling.md),
[text](text-editing.md), [application activation](application-activation.md) and
[application catalog](application-catalog.md). Those guides describe the code at
their stated source; delivery and acceptance must be identified separately.

## Accepted runtime and pending acceptance

| Cohort | Implemented scope | Delivery and acceptance boundary |
| --- | --- | --- |
| Text `8e6d074b` | Shared glyph/source geometry, script/fallback shaping and ordinary IME, plus hard-paragraph levels across wraps, exact row-bound resolved context, line-local L1/L2, tab separator projection and a stale-context cache guard. | Merged through PR87, source-reviewed and deployed. Focused formatting/Clippy, six wrapped UI, one upstream reference, six panel and two 1×/2× Vulkan checks pass. Final full workspace/protocol/832-case CI and the coherent release build also pass at `8e6d074b`; five owned native feature checks and all five restoration checks pass in the fresh controlled suite. Third-party IME and wider text acceptance remain pending. Earlier evidence retains its own source scope. The [text guide](text-shaping.md) and [typography review](text-baseline-review.md) preserve those sources. |
| Application `bb304907` | Domain open/context values, XDG adapter dispatch/revalidation and generic opt-in Wayland activation effects; standalone intent consumer and owned native harness. | Merged through PR87 and included in deployed `8e6d074b`. With the independent `d1fff2d8` consumer, now merged through PR89, a later scoped Chromium run passes nine feature and eight restoration checks, including observed focus, fresh local documents and launch-context isolation. [Activation](application-activation.md) identifies the separate consumer source and remaining application/token-consumption boundary. |
| Documentation/native-harness base `cd883719` | Includes guarded tools for real applications, GUI clipboard, third-party tray, simultaneous outputs and suspend/recovery, plus the reviewed IME lifetime/input guards from `6282122f`. That snapshot compiled runtime `8e6d074b`. | Tool existence alone is not native acceptance. The later `f16cb591` simultaneous-output harness passes its scoped 16-check native run on production `8e6d074b`. The `2044e54f` clipboard harness also passes nine real Chromium/Notes keyboard-text and twelve restoration checks on unchanged `8e`, one virtual output. The genuine wired-only tray run separately passes five interaction, four cleanup and thirteen post-removal preservation checks on unchanged `8e`. Other workflows, broader tray/clipboard apps/content/races, physical devices and hardware pacing remain pending. |

Exact `8e6d074bfcb30a0bcc0b371afddcd50c08993af0` passes both
[push CI](https://github.com/xandreeddev/desktop-lab/actions/runs/39313647281) and
[PR CI](https://github.com/xandreeddev/desktop-lab/actions/runs/39313659557): all six
checks, including full workspace, six controlled service-protocol filters and
832/832 desktop visual cases. One coherent locked/offline release builds desktop,
CLI, menu, lock, greeter and polkit together with international-text,
input-method-peer and application-intent. All nine artifacts are archived by
hash. The private archive manifest SHA256 is
`b7a1d92c4e0e186fca4e2e7e67523763c0c57bc4b91bd3ef18247c5b5cc996c9`.
Build success is separate from deployment and native acceptance.

The later guarded deployment verifies all six managed user clients and the root
greeter as one coherent `8e6d074b` release; only the changed compatibility helper
is replaced. Original settings, media, exports, clipboard and security configuration
remain preserved, greetd is not restarted, and rendered supervision continues
beyond its startup deadline. The three acceptance executables are staged privately,
not installed as user applications. The successful private receipt is
`reports/local/daily-text-8e-deployment-33ba0219a8a4a8a4c924f896.json`.

The fresh mapped-peer IME suite uses the merged `8e` desktop/example and
independent peer `3beda617`, with harness `6bb80e5942` and shared unit observer
`89604f6ef2`. All five feature checks pass: preedit adds 70 non-caret ink pixels
with 42-pixel caret advance before commit, CJK commit/surrounding-delete/history,
masked-focus composition retirement, multiline commit/cursor rectangle and native
RTL visual navigation. All five restoration checks also pass: exact fixture
units retire, the owned peer transport closes, settings/wallpaper and
output/workspace remain preserved, and the original pointer is restored. The
host-observed bounded run takes 108.776 seconds; this is no timing benchmark.

The [sanitized report](../reports/measurements/lucent-international-text.json)
records this owned one-output acceptance. It does not establish third-party IME,
physical-device, cross-output text or GUI clipboard behavior. The earlier
five-feature run failed peer-unit cleanup, and its separate guarded recovery
passed; both keep their original flags. Earlier failed-before-composition runs
also remain historical. See [the native text evidence](native-text-acceptance.md).

The later AMD native-context trial passes RADV RENOIR enumeration, secure login,
Lucent hardware-adapter selection and advancing native frame counters over three
seconds at the older deployed `a7f41ecc` binary. Mount-anchored preservation of
user/source/settings/media/critical files and the monitor passes, and the
candidate is retained. Its receipt is private
`reports/local/daily-amd-native-radv-liveness.json`. The earlier Btrfs device-remap
preservation failure and its recovery remain historical evidence, as does the
failed Venus trial. See [the dated graphics report](lucent-vulkan.md).

That earlier trial establishes hardware-path liveness on its named binary; the
later `8e6d074b` deployment and resource observation have their own scope. The
later App run passes scoped Chromium launch/focus/local-file Open/action/socket/CLI
and cleanup with the independent `d1fff2d8` consumer, now merged through PR89, on unchanged `8e` desktop/CLI.
A later [real GUI clipboard run](../reports/measurements/lucent-native-gui-clipboard-2044.json)
with harness `2044e54f` passes nine interaction and twelve restoration checks for
Chromium textarea ↔ Notes keyboard text transfers on unchanged `8e`, one virtual output.
Third-party IME, other applications and remote URI handlers, broader GUI clipboard
content/races, broader third-party tray applications/menus,
physical-device and instrumented presentation acceptance remain pending. The
later [two-output report](../reports/measurements/lucent-native-multi-output.json)
records its separate owned-headless native density/input/removal acceptance;
physical monitors, secure-output changes and suspend/resume remain unverified.

## Source-specific native evidence

Each report identifies its tested source. Display is now merged through PR86;
its native inputs still belong to the recorded `a7f41ecc` binary and harness
cohort, not every newer combined source. These are scoped observations, not fresh
executions caused by editing this guide.

| Behavior and source | Observed acceptance | Remaining boundary |
| --- | --- | --- |
| [Saved Wi-Fi recovery, `c17d6e7d`](../reports/measurements/lucent-wifi-credential-recovery.json) | Fourteen native NetworkManager and panel checks against owned WPA2/hidden simulated radios: incorrect-password cancellation, missing-secret masked repair on the same UUID without duplicates, DHCP, subsequent prompt-free reconnect, targeted disconnect/Forget and original routes/profiles/radio/preferences restored. | Physical Wi-Fi, SAE/OWE/enterprise/WPS, agent-owned vaults, captive portals and suspend remain unverified. Executable source is `c17d6e7d`; final documentation head `0c7c8e98` merged in PR82. |
| [Displays, `a7f41ecc`](../reports/measurements/lucent-display-settings.json) | Seven actual native input flows: compatibility open/Escape, draft-only mode, mode Apply/Revert, scale timeout, position Keep, original file/native restoration and deliberate UI-crash EOF rollback. All file hashes/identities are exact; the expected one-restart process transition passes explicit unit/process/recovery validation. | One QEMU output. Actual input source `c43df300`; offline validator `b259ea29` uses captured evidence plus one read-only probe. The original generic fingerprint assertion failed after the expected restart and is retained; this contract correction does not replay the inputs. Physical/multiple outputs, native enable/disable/rotation, hotplug/suspend and secure-output acceptance remain open. |
| [Two simultaneous outputs, `8e6d074b`](../reports/measurements/lucent-native-multi-output.json) | 16 native checks with harness `f16cb591`: original output plus one owned headless output, real audio-panel clicks on both at 1×/2×/1.25×, held-button removal, scoped host/surface retirement, same-name new global/generation and real input afterward. Original monitor/workspace/cursor, supervisor, configuration/settings/referenced media and idle marker restore exactly. | Owned QEMU output workload. No physical cable/display, secure-output, suspend, multi-output Display settings transactions, cross-output text/clipboard or presentation/resource acceptance. Earlier coordinate and shared-dock assertion failures remain failed reports. |
| [Two-output Displays, harness `86df4e5b` / runtime `35ed87ed`](../reports/measurements/lucent-native-two-output-displays.json) | Four native flows: draft-only peer position, Apply/explicit Revert, scale-1.25 independent timeout and position Keep with complete two-output persistence. The original 1920×1080 scale-1 output stays unchanged; the peer is 1280×960. Five positive owner checks plus independent owner retirement and fresh normal-OFF verification confirm original monitor bytes/inode, preferences, supervision, output/workspace/cursor restoration. | All input remains on the original 1× panel. Monitor ctime/link count and parent entry timestamps can advance. Initial `7ebd0e84` failed with zero checks because eval did not dispatch a cursor move; its failure and separate recovery remain unchanged. No physical/secure/hotplug/suspend, native enable/disable/rotation, fractional-panel input, resource or pacing acceptance. |
| [Owned international text, `8e6d074b`](../reports/measurements/lucent-international-text.json) | Five native feature checks and five restoration checks pass with harness `6bb80e5942`, owned-unit observer `89604f6ef2` and mapped peer `3beda617`: real preedit ink/caret advance, CJK commit/delete/history, masked-focus retirement, multiline composition and RTL navigation; exact fixtures/transport retire and original settings/wallpaper/output/workspace/pointer restore. | One owned non-replacing peer and one prepared output. Third-party IMEs, physical/cross-output text, GUI clipboard, secure authentication and broader pixel/scroll/focus-race acceptance remain separate. Earlier failed reports retain their original flags. |
| [Real Chromium intents, unchanged `8e6d074b`](../reports/measurements/lucent-native-application-intents-d1.json) | Nine feature and eight restoration checks pass with independent consumer `d1fff2d8`, now merged through PR89, and harness `b6323e9`: first-sample native browser focus with consumer keyboard None, fresh physical-token child delivery, two new exact local documents per physical Open/action/socket/CLI request, no stale token reuse, target-specific close and exact owned artifact/caller restoration. | One prepared output and real owned Chromium profile. No compositor forced focus, direct Chromium token-consumption proof, other-application/remote-URI, physical hardware or broader launcher/catalog/dock acceptance. The earlier failed partial report remains unchanged; deployed desktop/CLI and the coherent production archive remain `8e`. |
| [Real Chromium ↔ Notes clipboard, unchanged `8e6d074b`](../reports/measurements/lucent-native-gui-clipboard-2044.json) | Nine interaction and twelve restoration checks pass with harness `2044e54f`: actual keyboard copy/paste in both directions, selected-text replacement, multiline/combining/emoji/CJK and blank/trailing-line preservation; exact original user state, owned browser/checkpoint/profile retirement and 35-second normal desktop stability. | One virtual output and an owned Chromium textarea. No rich HTML/images, other apps, delayed-provider/focus-retirement races, physical devices, IME/shaping-pixel, host-crash automatic restoration or performance acceptance. Earlier failed attempts remain unchanged. |
| [Notes, `3806bcfc`](../reports/measurements/lucent-multiline-notes.json) | 17 functional and nine restoration checks: native editing/navigation/selection, grapheme deletion, undo, copy/cut/paste with `wl-copy`/`wl-paste`, resized editing geometry, restart persistence and stable restored supervision. | One output. No third-party GUI clipboard interoperability or general native pixel-level caret/selection/clipping proof, scrollbar-thumb drag, exact soft-wrap/scroll affinity, Page Up/Down/edge-autoscroll or delayed clipboard/focus-retirement race acceptance. The report retains an initial restoration failure and the later successful fresh run. |
| [Genuine NetworkManager Applet, unchanged `8e6d074b`](../reports/measurements/lucent-native-third-party-tray-8e.json) | Five native interaction and four cleanup checks pass: exact real applet ownership, its item in Lucent’s native bar, themed DBusMenu input and the genuine Connection Information window. Thirteen fresh outside-worker checks after normal removal restore all 964 original packages/reasons, leave five test packages absent, and preserve network/user files/runtime/security/autostart authority. | One wired-only virtual output. Controlled directory sizes/timestamps may advance while directory authority and non-directory records remain exact. No other apps, Wi-Fi/secret-agent, pairing, nested-menu, physical hardware, pixel quality, FPS or resource acceptance. |
| [Tray, `b8a69c8f`](../reports/measurements/lucent-tray-bar.json) | Owned SNI/DBusMenu fixture with actual VM pointer/keyboard input: primary/middle/vertical-wheel actions, right-click menu, menu-only fallback, overflow, item status/owner retirement and stale-reply handling. | No third-party application compatibility. Application-owned popups use neutral position hints; horizontal/high-resolution wheel coverage is automated rather than this native test. No XEmbed or hardware/performance claim. |
| [Live display fit, `83a02372`](../reports/measurements/lucent-live-display-fit.json) | One-output live video follows actual resize, integer/fractional density and rapid target changes; old producer generations retire and fitted frames continue. Original output/preferences/source restore. | No simultaneous-output placement/input, alpha-safe fitting, seamless playhead retargeting, hardware decode or actual 60 FPS presentation proof. Software resource targets remain unmet. |
| [Weather, `579c62cb`](../reports/measurements/lucent-weather.json) | Strict/visual checks, one-output native opening/focus routing, passive center bar, panel switching/closing, Tab-only field focus and one production forecast. | No native form edits/Apply/unit changes, field clipboard/selection/scroll, provider failure/retry or precise animation acceptance. The separate deployment intentionally performed the guarded legacy settings migration; the subsequent read-only interaction cohort preserved its own entry-state settings. |
| [Live previews, `ba693fb9`](../reports/measurements/lucent-live-wallpaper-previews.json) | One 1× output and owned synthetic GIF: changing preview crops, select-only browsing, Tab/wheel, same-path Retry/Refresh, close/reopen, explicit Apply and exact caller restoration. Full 796 software-Vulkan comparisons pass at the recorded source. | No preview-specific RSS, decoder concurrency measurement, scanout/smoothness, multiple-output, fractional, codec or hardware claim. |
| [System redraw routing, `82a27a46`](../reports/measurements/lucent-system-widget-invalidation.json) | During the recorded 13.765-second one-output native case, standalone System submits three times, live background 261 times, and unrelated Calendar/Clock submit zero times. | This proves scoped invalidation routing. It does not measure CPU/RSS savings, GPU execution or presentation rate. |
| [Bluetooth rejection, `3ff2aa3a`](../reports/measurements/lucent-bluetooth-rejection.json) | Two software HCI controllers: native comparison-code pairing/bonding, explicit decline/Escape termination, unpaired Disconnect and compatibility routing, without automatic trust. | Physical radio/profile compatibility, incoming/service authorization and native pending-profile/PIN-entry acceptance remain open. [The later connection report](../reports/measurements/lucent-bluetooth-connection.json) records protocol/client coverage without extending native profile acceptance. |

Single-output native tooltips, earlier playback/lock workflows and virtual audio
also have their own evidence. See the feature reports rather than treating this
table as a replacement for every acceptance record.

## Reading resource and animation claims

The recorded resource trial at `35ed87ed` combines decoder-session
reuse and the bounded generic stream-upload pool. Its coherent nine-binary build
and guarded deployment verify all six managed user clients and the root greeter,
supervision beyond startup and preserved user/security state. Settings/theme are
not changed and greetd is not restarted. The renderer’s source-specific strict
Clippy, two pure and five headless Vulkan correctness/lifecycle tests pass; the
separate offscreen CPU diagnostic observes effectively similar pooled/fallback
cost. None of those checks establishes a native performance gain.

The normal uninstrumented interval records **120.000419 s**, 480 samples at
0.25 s spacing, **250.580664 MiB mean / 252.394531 MiB sampled peak RSS**,
**154.032796% CPU of one logical core** and zero guest-process swap. Preservation
passes. Host 2916 swap-in pages and I/O full-stall fraction 0.491988 make this
interval **ineligible**, with normal-budget acceptance null despite observed
RSS below 300 MiB. CPU and presentation tracing are off in this resource phase.

A separate opt-in metadata interval joins all **3934 distinct prepared resource
revisions** referenced by successful background feedback. Its **3933 advances /
122.049995 compositor seconds = 32.224499 advances/s**; feedback cadence is the
same because no revision repeats. Presentation gap p50/p95/p99/max is
**33.333/33.334/50/83.333 ms**. One clock and flags `7` are consistent throughout;
there are no ambiguous/missing joins or truncation. These bound references do
not establish final pixel visibility or physical scanout.

Separately, 134 host-monotonic RPC observations bound **7322 post-conversion,
ImageStream-prepared callbacks** to **59.733251–60.129012 callbacks/s**. This is
wall-time counter evidence before mailbox publication, not media-PTS-as-wallclock,
decoder/input FPS or compositor presentation. The difference between stages does
not identify a bottleneck or count decoder drops. The metadata interval is also
**ineligible**, with 1926 host swap-in pages and I/O full-stall fraction 0.548044,
plus an ineligible resource reference and unmatched I/O pressure. No accepted
60 FPS, resource target or causal comparison follows.

Profiling restoration passes, and fresh independent read-only facts confirm
CPU, presentation and source tracing are off. This restarts controlled service
lifetimes; it is not a same-PID or shared-directory-timestamp preservation claim.
See the [sanitized upload-trial report](../reports/measurements/lucent-live-upload-35ed-native-trial.json)
for hashes, exact windows, flags, preservation and limits.

The separate archived `frame-pacing` example at `85a5be63` uses the public
component API: `view` reads `ViewContext.now` to move a rounded rectangle, while
`Application::animating` requests another compositor-available frame. The shared
[FrameDemand API](../lucent/crates/lucent-api/src/lib.rs) retains dirty work while
a native callback is pending. There is no media producer, source clock, image or
stream upload. During that diagnostic the managed desktop stayed on `35ed`; its video was paused and
resumed through ordinary commands for this bounded diagnostic.

The [shape report](../reports/measurements/lucent-native-shape-diagnostic.json)
records **4468 selected feedbacks / 4467 intervals over 120.566662 compositor
seconds = 37.050043 intervals/s**, with four execution and five restoration checks
passed. The measurement loop lasts 120.781719 wall seconds. Its simpler shared
native path also records below 60 in this run; that does not locate a bottleneck
or establish a compositor/driver/VM cap. No host-pressure or resource collector
ran. One private static image confirms only partial shape visibility beneath
separate widgets, with no smoothness, whole-surface pixel or scanout acceptance.
That report records accepted main `9129912b` at its own preparation time. The
archived example is also distinct from later accepted main `2b3c6db4`; neither
diagnostic is a new full-shell deployment.

A later [CPU-profiled shape run](../reports/measurements/lucent-native-shape-cpu-diagnostic.json)
reuses the same archived `85a5` binary while keeping managed `35ed` normal with its
profiling off. After 30 seconds of warmup, **5849 feedbacks / 5848 intervals over
120.899995 compositor seconds = 48.370556 intervals/s** are recorded. All feedbacks
join CPU rows; two additional terminal CPU rows are outside the saved feedback
set, without lost history. All four execution and five restoration checks pass;
a fresh independent normal-state capture confirms the same desktop lifetime and
all three profiling flags off. One private static image confirms partial visibility.

The **5851 successful draw rows average 0.658506 ms**, with p95 **0.946377 ms**,
p99 **1.145934 ms** and maximum **2.480036 ms**. Acquire averages 0.110190 ms and
present 0.118709 ms, with maxima 0.323032/1.784016 ms. No image/text misses or
texture uploads are recorded. These are CPU wall times, including driver waits,
not GPU execution or a full-loop profile. Present includes protocol requests and
the presentation call; separate feedback reports compositor timestamps. The exact
diagnostic unit journal also records Mailbox, with Fifo and Immediate supported;
this is an observed mode, not a mode-comparison or pacing-cause result. Application
updates, callback/dispatch delay and unsuccessful retries are outside these rows.
The remaining cadence gaps are unattributed. No host-pressure or RSS collector
ran, and different intervals/profiling prevent a causal comparison with the earlier
37.05/s shape or 32.22/s video observations. No 300 MiB or 60 FPS acceptance follows.

Separate operational cleanup archives and removes thirteen already-retired test
payloads: nine owned browser profiles and four staged binaries. Observed tmpfs
availability increases by 604.055 MiB. All twelve metadata directories and three
unleased profiles remain; the disk archive is fully verified and retained.
Independent checks preserve the normal source/settings/security/lifetime/output
state and confirm profiling off; parent-directory timestamps advance. This is
maintenance, not a before/after resource or pacing experiment.

To prevent new browser acceptance fixtures from refilling RAM-backed temporary
storage, test source `842fbc9b` gives the two browser suites a shared allocator
under private disk-backed cache storage. It rejects tmpfs/ramfs and unsafe or
symlinked parents, requires 1 GiB available at admission, and creates fresh
`0700` directories. That reserve is not a profile-size cap; failed or uncertain
fixtures remain on disk for review, with no automatic pruning. Existing lease
and unit-retirement guards remain unchanged. The [storage report](../reports/measurements/lucent-disk-backed-browser-fixtures.json)
records 531 local Python checks, including eight allocator tests. On unchanged
`35ed`, a fresh singular HTTP run passes twelve feature and nine cleanup checks;
the real Chromium/Notes keyboard clipboard run passes nine feature and twelve
restoration checks. Original settings, clipboard history, wallpaper, layout and
session state restore, with one expected guarded desktop restart and 35 seconds
of stable supervision. A fresh independent normal-state capture confirms the
same runtime source with all three profiling flags off. This does not claim the
same PID, other applications, rich content or a resource/pacing improvement.

The earlier [bf98 decoder-session report](../reports/measurements/lucent-live-decoder-session-native-trial.json)
and [9b trial](../reports/measurements/lucent-live-threading-9b-native-trial.json)
retain their own resource results and limitations. The bf98 normal observation
has no presentation interval. The following accepted-baseline 8e observations
also retain their original source; none is a matched causal comparison with 35ed.

The accepted-baseline `8e6d074b` RADV cohort records an uninstrumented 120.000492-second
interval with 480 RSS samples at 0.25-second spacing: **217.28 MiB mean /
221.92 MiB sampled peak**, 112.25% CPU of one logical core and zero guest-process
swap. The unchanged 4K60 H.264 video, source-rate setting and single 1080p60 output
pass the full preservation checks. Process-lifetime high-water RSS is 223.75 MiB,
separate from the sampled interval peak.

This observation is **ineligible for the 300 MiB acceptance gate**: the host
records 674 swap-in pages and an I/O full-stall fraction of 0.5081; host available
memory stays above 4152.29 MiB. The retained result has `eligible=false` and
`normal_uninstrumented_rss_target_met=null`, with no automatic retry. Profiling was
off in that reference; it contains no PTS/presentation samples. Its private
normal-resource wrapper report is
`.cache/final-cohort-measurement-v2/measurement-8e-radv-1792442023-resources.json`.
It establishes neither accepted resource targets, a matched improvement over
llvmpipe nor actual FPS, animation pacing, GPU memory or hardware decoding.

A later, separately instrumented native run on the same frozen 8e binary completed
120.000563 resource seconds. Its selected background history contains 3276 valid
feedback/source joins over 121.299995 compositor seconds: **26.9992 distinct
prepared-revision advances/s**, with interval p50 **33.333 ms**, p95 **50 ms**,
p99 **83.333 ms**, and maximum **150 ms**. All selected joins are unique; none is
a repeated prepared revision. The full retained 3397-background history includes
121 requests before the observation boundary and must not be divided by 120.
The [offline guide](native-presentation-analysis.md) and
[sanitized report](../reports/measurements/lucent-native-presentation.json)
explain the exact request windows, integer-nanosecond calculations and
cycle-aware PTS gaps. The prepared catalogue already has PTS gaps, but attribution
to decode, conversion, host pressure or presentation requires stage evidence.

That instrumented sample records 219.4954 MiB mean / 219.4961 MiB peak RSS and
113.6495% of one logical core, without guest-process swap. Host 610 swap-in pages,
I/O full-stall fraction 0.5662336 and unmatched reference pressure make it
**ineligible** too. It does not establish normal RSS-budget acceptance or a
causal instrumentation-overhead comparison. The 60 FPS target was not met in this
run, while eligible performance, pixel visibility and physical scanout remain
unestablished. The separate V3 restore passed 42.774 s, returning flags off and
fresh normal desktop/helper lifetimes stable for 35 s while preserving original
source/settings/media/security; no same-PID preservation is claimed. Earlier
failed V1/V2 profiling attempts keep their original results.

The [Display report](../reports/measurements/lucent-display-settings.json) records
the separate 120-second normal-video sample of the same `a7f41ecc` binary:
384.82/403.88 MiB mean/peak RSS, 101.466% of one core and zero swap. A 4K H.264
60 FPS source is fitted to one 1080p scale-1 output with source-rate playback;
source, settings, media, output and playback state remain exact. This uninstrumented
resource sample proves neither actual 60 FPS presentation nor an isolated speedup.
The [Weather report](../reports/measurements/lucent-weather.json) retains the
earlier 424.12/424.15 MiB sample at its different source/workload cohort. The
[live-fit report](../reports/measurements/lucent-live-display-fit.json) records a
separate matched physical-cover conversion comparison. Older static or lower-rate
video reports remain historical evidence; their smaller numbers do not establish
the accepted-main binary's current RSS.

Prepared CPU pixels, cached texture payload, process RSS/PSS and GPU/driver
allocations measure different things. Successful submission counters and
compositor frame callbacks do not establish monitor presentation or 60 FPS.
Preserve physical density, antialiasing, effects and motion while measuring
normal idle, animated, cold and warm workloads. Historical samples used software
Vulkan; the later RADV resource observation does not change those measurements
or supply physical-GPU pacing or hardware-decode acceptance.

## Maintaining this reconciliation

Keep domain contracts named ports and concrete implementations named adapters.
Record implementation, automated checks, native interaction and hardware/app
acceptance separately. Update a feature's current summary only after its source
and evidence are accepted; preserve the original report and chronological ledger
entries. A screenshot, a CLI acknowledgement or a command's admission does not
prove the underlying OS operation completed.

This guide distinguishes the recorded Display input run, its explicit offline
acceptance-contract correction and the separate same-binary read-only resource
sample. Historical ledger entries remain intact. Final Astro/site checks and
exact-head CI are separate documentation delivery gates; the seven requested gaps
are not all closed. This reconciliation does not run native or hardware acceptance.
