# Current daily-use readiness

Accepted main for this reconciliation is `d00014509f13125fe4ccab3ce2e1fa3961a40dd3`
(PR86, including the verified Display milestone), dated 2026-10-19. Its Display
executable source is `a7f41ecc`; the documentation/native-tool snapshot is
`d0611935`. Implemented, unmerged Text and Application cohorts are identified
separately below and are not counted as delivered. Publication and deployment
remain separate from merging. The seven requested gaps all remain in scope.
Accessibility is separately deferred by the user.

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
| International text | Accepted main retains controlled editing/clipboard and the earlier shaping path. Unmerged Text `5d3501eb` implements script shaping, paragraph bidi, visual source/caret maps, deterministic fallback and opt-in text-input-v3 composition; recorded strict/pixel/proxy checks have their own source scope. | Merge/deployment, wrapped mixed-direction paragraph/isolate correctness and native compositor IME acceptance remain pending. Native pixel/scroll details, delayed clipboard/focus races and third-party GUI clipboard still need acceptance; controlled protocol checks do not establish those results. |
| Application integration | Accepted main includes catalog, pins, ordinary activation and declared actions. Unmerged Application `bb304907` implements fresh input-bound activation context, explicit file/URI Open and expanded XDG parsing/observation, with a prepared real-application harness. | Merge/deployment and native third-party launch/open/action acceptance remain pending. An intent token, dispatch acknowledgement or prepared harness does not prove focus, document loading or application interoperability. |
| Real-world validation | Several owned native workflows already pass, as listed below. | Simultaneous monitors, hotplug, suspend/resume, physical Bluetooth/audio and other power/radio hardware, third-party tray applications and GUI clipboard interoperability need broader testing. Implementation and an owned fixture are separate from this acceptance. |
| Documentation accuracy | This reconciliation updates current summaries and repository skills against source-specific evidence. | Keep summaries and the ledger current when later PRs merge; site validation is required for this documentation change. Historical evidence must retain its original scope. |

The relevant contract guides are [network](native-network.md),
[safe display settings](native-displays.md), [output profiles](monitor-layouts.md), [density](fractional-scaling.md),
[text](text-editing.md), [application activation](application-activation.md) and
[application catalog](application-catalog.md). Those guides describe the code at
their stated source; delivery and acceptance must be identified separately.

## Implemented branches and pending acceptance

| Cohort | Implemented scope | Delivery and acceptance boundary |
| --- | --- | --- |
| Text `5d3501eb` / executable checks `eec582ab` | Shared glyph/source geometry, bidi, script itemization, deterministic Noto fallback, ordinary IME preedit/atomic commit, seat retirement and secure-client opt-out. | Implemented and controlled checks recorded; unmerged. The [text guide](text-shaping.md) and [typography review](text-baseline-review.md) identify exact sources and visual-reference changes. Native compositor IME is pending. |
| Application `bb304907` | Domain open/context values, XDG adapter dispatch/revalidation and generic opt-in Wayland activation effects; standalone intent consumer and owned native harness. | Implemented, unmerged. Native launch/open/action proof has not run; [activation](application-activation.md) separates physical input from socket commands. |
| Native-tool snapshot `d0611935` | Source includes guarded acceptance tools for real applications, GUI clipboard, third-party tray, simultaneous outputs and suspend/recovery. | Tool existence is not acceptance. Those native cases, physical devices and hardware pacing remain pending. |

The prepared GPU candidate has not executed at this documentation snapshot. No
hardware result supersedes the software-Vulkan RSS failure below. Resource probes,
GPU enumeration, native presentation and smooth 60 FPS are separate evidence.

## Source-specific native evidence

Each report identifies its tested source. Display is now merged through PR86;
its native inputs still belong to the recorded `a7f41ecc` binary and harness
cohort, not every newer combined source. These are scoped observations, not fresh
executions caused by editing this guide.

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
exact-head CI are separate documentation delivery gates; the seven requested gaps
are not all closed. This reconciliation does not run native or hardware acceptance.
