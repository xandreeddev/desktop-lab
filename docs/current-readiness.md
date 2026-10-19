# Current daily-use readiness

This is a reconciliation of accepted-main documentation base
`bc3561439504c8818b17b9b739480e54d1e351fe` plus the Display milestone contained
in this source, dated 2026-10-19. Its compiled source is `a7f41ecc`; merge and
publication remain separate delivery gates. Other unmerged feature work is not
counted as delivered. The seven requested gaps all remain in scope. Accessibility is
separately deferred by the user.

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
| Resources and animation | Event-driven redraw, bounded image/preview ownership and live-video physical-cover conversion are implemented. | The separate 120-second normal-video sample of the Display binary measures 384.82 MiB mean / 403.88 MiB peak RSS and 101.466% of one core on llvmpipe, above the 300 MiB target. This differs from the Weather cohort and establishes no isolated improvement. Actual 60 FPS presentation and consistent cold/warm animation timing are not established. |
| International text | Controlled ordinary/multiline editing, grapheme navigation, undo/redo and bounded clipboard transfers exist. | Complex-script shaping, bidirectional layout, font fallback and IME are unfinished in accepted main. Native pixel/scroll details and clipboard races need further acceptance. |
| Application integration | Live XDG catalog observation, identity-preserving selection, persisted pin/reorder, ordinary activation and desktop-entry action paths exist. | Fresh activation authority, file/URI opening and remaining desktop-entry handling need completion. Private protocol/client action checks do not establish third-party native activation interoperability. |
| Real-world validation | Several owned native workflows already pass, as listed below. | Simultaneous monitors, hotplug, suspend/resume, physical Bluetooth/audio and other power/radio hardware, third-party tray applications and GUI clipboard interoperability need broader testing. Implementation and an owned fixture are separate from this acceptance. |
| Documentation accuracy | This reconciliation updates current summaries and repository skills against source-specific evidence. | Keep summaries and the ledger current when later PRs merge; site validation is required for this documentation change. Historical evidence must retain its original scope. |

The relevant contract guides are [network](native-network.md),
[safe display settings](native-displays.md), [output profiles](monitor-layouts.md), [density](fractional-scaling.md),
[text](text-editing.md), [application activation](application-activation.md) and
[application catalog](application-catalog.md). Those guides describe the code at
their stated source; delivery and acceptance must be identified separately.

## Source-specific native evidence

Each report identifies its tested source. Earlier reports belong to accepted
main; the Display source is contained in this milestone and still has separate
PR delivery gates. These are scoped observations, not fresh executions caused
by editing this guide.

| Behavior and source | Observed acceptance | Remaining boundary |
| --- | --- | --- |
| [Saved Wi-Fi recovery, `c17d6e7d`](../reports/measurements/lucent-wifi-credential-recovery.json) | Fourteen native NetworkManager and panel checks against owned WPA2/hidden simulated radios: incorrect-password cancellation, missing-secret masked repair on the same UUID without duplicates, DHCP, subsequent prompt-free reconnect, targeted disconnect/Forget and original routes/profiles/radio/preferences restored. | Physical Wi-Fi, SAE/OWE/enterprise/WPS, agent-owned vaults, captive portals and suspend remain unverified. Executable source is `c17d6e7d`; final documentation head `0c7c8e98` merged in PR82. |
| [Displays, `a7f41ecc`](../reports/measurements/lucent-display-settings.json) | Seven actual native input flows: compatibility open/Escape, draft-only mode, mode Apply/Revert, scale timeout, position Keep, original file/native restoration and deliberate UI-crash EOF rollback. All file hashes/identities are exact; the expected one-restart process transition passes explicit unit/process/recovery validation. | One QEMU output. Actual input source `c43df300`; offline validator `b259ea29` uses captured evidence plus one read-only probe. The original generic fingerprint assertion failed after the expected restart and is retained; this contract correction does not replay the inputs. Physical/multiple outputs, native enable/disable/rotation, hotplug/suspend and secure-output acceptance remain open. |
| [Notes, `3806bcfc`](../reports/measurements/lucent-multiline-notes.json) | 17 functional and nine restoration checks: native editing/navigation/selection, grapheme deletion, undo, copy/cut/paste with `wl-copy`/`wl-paste`, resized editing geometry, restart persistence and stable restored supervision. | One output. No third-party GUI clipboard interoperability, native pixel-level caret/selection/clipping proof, scrollbar-thumb drag, exact soft-wrap/scroll affinity, Page Up/Down/edge-autoscroll or delayed clipboard/focus-retirement race acceptance. The report retains an initial restoration failure and the later successful fresh run. |
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
normal idle, animated, cold and warm workloads. The VM uses software Vulkan for
the cited samples; no physical-GPU pacing or hardware-decode result is implied.

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
exact-head CI are separate delivery gates; the seven requested gaps are not all closed.
