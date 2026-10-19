# Current daily-use readiness

Accepted main for this reconciliation is `b474d1fe845a7cfd716daf7c382de0769fbf3849`
([PR87](https://github.com/xandreeddev/desktop-lab/pull/87)), dated 2026-10-19. PR87 merged normally after all six required checks
passed on exact `8e6d074bfcb30a0bcc0b371afddcd50c08993af0` and the PR was clean.
That merged runtime includes Text and Application integration and is already
deployed coherently on RADV RENOIR. Merging, deployment and native acceptance
remain separate facts. The documentation/native-tool branch is newer than the
compiled runtime; editing its guides does not rebuild or repeat runtime tests.

The earlier three-second RADV liveness trial remains scoped to `a7f41ecc`.
A later 16-check native run on `8e6d074b` passes two-output density/input and
lifetime retirement with one owned headless output and full restoration. Five
owned IME feature checks also pass, including visible preedit/caret evidence,
but that run fails its cleanup gate; separate guarded recovery passes without
changing the original failed report. The normal resource observation remains
host-pressure-ineligible and presentation FPS is unproven. The seven requested
gaps all remain in scope. Accessibility is separately deferred by the user.

The shell's launcher, dock, widgets, notifications, lock/login and service panels
are implemented. Several daily-use cases and their real-world acceptance remain
unfinished. This guide corrects older “native testing pending” summaries where
later evidence exists; it does not declare the entire desktop ready.

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
| Safe display settings | Native whole-layout drafts, fresh ordered observations, Apply → Keep/Revert and independent 15-second/UI-crash EOF rollback. Seven actual one-output QEMU flows pass for mode, scale, position, persistence and recovery. File preservation is exact; the expected one-restart service transition is validated explicitly. | Physical/multiple outputs, native enable/disable/rotation, fractional display-settings input, hotplug, suspend/resume, compositor restart and secure-output acceptance remain open. The Keep-to-file-identity-capture host-loss interval retains manual recovery. |
| Resources and animation | Event-driven redraw, bounded image/preview ownership and live-video physical-cover conversion are implemented. The deployed `8e6d074b` RADV cohort records 217.28 MiB mean / 221.92 MiB sampled peak RSS over 120 seconds. | The result is ineligible for the 300 MiB acceptance gate because host swap activity and I/O full-stall pressure exceed the reviewed bounds. Presentation tracing was off: no FPS or smoothness result. Historical llvmpipe 384.82/403.88 MiB remains a different cohort, without a matched speedup comparison. |
| International text | Merged `8e6d074b` implements script shaping, hard-paragraph context across soft wraps, exact row-bound visual source/caret maps, tab separator projection, deterministic fallback and opt-in text-input-v3 composition. Five owned native feature checks pass: visible preedit/caret advance, CJK commit/delete/history, masked-focus retirement, multiline composition and RTL navigation. | The original native run remains failed at cleanup; separate independent guarded restoration passes. Full IME suite acceptance, broader native pixel/scroll details, delayed clipboard/focus races and third-party GUI clipboard remain pending. Exact merged `8e6d074b` passes six CI checks, full workspace, six controlled service-protocol filters and 832 Vulkan desktop cases; those checks retain their own scope. |
| Application integration | Accepted main includes catalog, pins, ordinary activation, declared actions and Application `bb304907` through merged/deployed `8e6d074b`: fresh input-bound activation context, explicit file/URI Open and expanded XDG parsing/observation. | Native third-party launch/open/action acceptance remains pending. An intent token, dispatch acknowledgement or prepared harness does not prove focus, document loading or application interoperability. |
| Real-world validation | Several owned native workflows already pass. The coherent `8e6d074b` also passes 16 checks for two simultaneous compositor outputs, one headless, including real panel input at 1×/2×/1.25×, held-button retirement, same-name recreation and exact restoration. | Physical monitors/cable hotplug, suspend/resume, secure-output changes, physical Bluetooth/audio and other power/radio hardware, third-party tray applications and GUI clipboard interoperability need broader testing. The owned output test establishes no presentation FPS or multi-output Display settings transaction. |
| Documentation accuracy | This reconciliation updates current summaries and repository skills against source-specific evidence. | Keep summaries and the ledger current when later PRs merge; site validation is required for this documentation change. Historical evidence must retain its original scope. |

The relevant contract guides are [network](native-network.md),
[safe display settings](native-displays.md), [output profiles](monitor-layouts.md), [density](fractional-scaling.md),
[text](text-editing.md), [application activation](application-activation.md) and
[application catalog](application-catalog.md). Those guides describe the code at
their stated source; delivery and acceptance must be identified separately.

## Accepted runtime and pending acceptance

| Cohort | Implemented scope | Delivery and acceptance boundary |
| --- | --- | --- |
| Text `8e6d074b` | Shared glyph/source geometry, script/fallback shaping and ordinary IME, plus hard-paragraph levels across wraps, exact row-bound resolved context, line-local L1/L2, tab separator projection and a stale-context cache guard. | Merged through PR87, source-reviewed and deployed. Focused formatting/Clippy, six wrapped UI, one upstream reference, six panel and two 1×/2× Vulkan checks pass. Final full workspace/protocol/832-case CI and the coherent release build also pass at `8e6d074b`; five owned native feature checks pass but overall native text cleanup/acceptance remains pending. Earlier evidence retains its own source scope. The [text guide](text-shaping.md) and [typography review](text-baseline-review.md) preserve those sources. |
| Application `bb304907` | Domain open/context values, XDG adapter dispatch/revalidation and generic opt-in Wayland activation effects; standalone intent consumer and owned native harness. | Merged through PR87 and included in deployed `8e6d074b`. Native launch/open/action proof remains pending; [activation](application-activation.md) separates physical input from socket commands. |
| Documentation/native-harness base `cd883719` | Includes guarded tools for real applications, GUI clipboard, third-party tray, simultaneous outputs and suspend/recovery, plus the reviewed IME lifetime/input guards from `6282122f`. Runtime compilation remains `8e6d074b`. | Tool existence alone is not native acceptance. The later `f16cb591` simultaneous-output harness passes its scoped 16-check native run on production `8e6d074b`; other workflows, physical devices and hardware pacing remain pending. |

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

The latest mapped-peer IME attempt uses the merged `8e` desktop/example and
independent peer `3beda617`. All five feature checks are true: real preedit
adds 70 non-caret ink pixels with 42-pixel caret advance before commit, CJK
commit/surrounding-delete/history, masked-focus composition retirement,
multiline commit/cursor rectangle and native RTL visual navigation. This is one
owned peer/output, not third-party IME or physical-device acceptance.

The run's overall result is **failed** because peer-unit cleanup encounters a
completed-unit metadata/garbage-collection assertion; original restoration flags
remain false. A separate independent guarded restoration verifies both captured
units/PIDs absent, the same desktop and exact settings/wallpaper/output/workspace,
only the captured refused socket retired, and original cursor restored. That
successful recovery does not turn the original run into a passed full suite.
The earlier failed-before-composition attempt remains historical. Cleanup tooling
is being corrected; its fresh native gate remains pending. See
[the native text evidence](native-text-acceptance.md).

The later AMD native-context trial passes RADV RENOIR enumeration, secure login,
Lucent hardware-adapter selection and advancing native frame counters over three
seconds at the older deployed `a7f41ecc` binary. Mount-anchored preservation of
user/source/settings/media/critical files and the monitor passes, and the
candidate is retained. Its receipt is private
`reports/local/daily-amd-native-radv-liveness.json`. The earlier Btrfs device-remap
preservation failure and its recovery remain historical evidence, as does the
failed Venus trial. See [the dated graphics report](lucent-vulkan.md).

That earlier trial establishes hardware-path liveness on its named binary; the
later `8e6d074b` deployment and resource observation have their own scope. Complete native
IME cleanup acceptance, real application launch/Open/action, GUI clipboard, third-party tray,
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
| [Notes, `3806bcfc`](../reports/measurements/lucent-multiline-notes.json) | 17 functional and nine restoration checks: native editing/navigation/selection, grapheme deletion, undo, copy/cut/paste with `wl-copy`/`wl-paste`, resized editing geometry, restart persistence and stable restored supervision. | One output. No third-party GUI clipboard interoperability or general native pixel-level caret/selection/clipping proof, scrollbar-thumb drag, exact soft-wrap/scroll affinity, Page Up/Down/edge-autoscroll or delayed clipboard/focus-retirement race acceptance. The report retains an initial restoration failure and the later successful fresh run. |
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

The deployed `8e6d074b` RADV cohort records an uninstrumented 120.000492-second
interval with 480 RSS samples at 0.25-second spacing: **217.28 MiB mean /
221.92 MiB sampled peak**, 112.25% CPU of one logical core and zero guest-process
swap. The unchanged 4K60 H.264 video, source-rate setting and single 1080p60 output
pass the full preservation checks. Process-lifetime high-water RSS is 223.75 MiB,
separate from the sampled interval peak.

This observation is **ineligible for the 300 MiB acceptance gate**: the host
records 674 swap-in pages and an I/O full-stall fraction of 0.5081; host available
memory stays above 4152.29 MiB. The retained result has `eligible=false` and
`normal_uninstrumented_rss_target_met=null`, with no automatic retry. Profiling is
off and no PTS/presentation samples are collected in that interval. A separate
profiling-enable operation does not establish a presentation measurement. Its
private normal-resource wrapper report is
`.cache/final-cohort-measurement-v2/measurement-8e-radv-1792442023-resources.json`.
It establishes neither accepted resource targets, a matched improvement over
llvmpipe nor actual FPS, animation pacing, GPU memory or hardware decoding.

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
