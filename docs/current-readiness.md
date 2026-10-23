# Current daily-use readiness

The current integration base is accepted main
`124b206938ebc3b41f5720838d01b8edf49a438d`
([PR100](https://github.com/xandreeddev/desktop-lab/pull/100)). It merged normally
at 2026-10-22 16:24:21 UTC after all six required checks passed on exact
`efeaf6d177161a2825cbb283f14b721d27afac8a` with CLEAN mergeability.
[Pages run 39813782003](https://github.com/xandreeddev/desktop-lab/actions/runs/39813782003)
publishes that merge. Public desktop, library and component-lab HTML, the native
capture manifest and a representative PNG match the deployed artifact. All 114
capture-producing inputs also match the recorded source and merged tree.

The core Wi-Fi repair, safe display editing, international text and explicit
application-opening paths are implemented with scoped native evidence. The
seven-gap goal is **not complete**: eligible ≤300 MiB and source-rate presentation
measurements, physical devices and broader application interoperability remain
open. Delivery and a green test suite do not establish those results.

The recent changes add:

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
accepted `8e6d074b` runtime evidence, later unmerged `35ed87ed` resource trial and
`8a3cd7d8` candidate workflows retain separate identities. No candidate is
promoted to accepted main by a documentation edit. Earlier delivery receipts,
failed attempts and their independent recoveries remain in
[the ledger](implementation-status.json) and the source-specific reports below.
Accessibility remains user-deferred.

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
| Safe display settings | Native whole-layout drafts, fresh ordered observations, Apply → Keep/Revert and independent 15-second/UI-crash EOF rollback. Seven earlier one-output flows pass. The later `86df4e5b` harness on unchanged `35ed` passes four two-output flows: draft-only position, Apply/explicit Revert, peer scale-1.25 timeout and position Keep. The original 1920×1080 scale-1 output stays unchanged; the owned peer is 1280×960. Original monitor bytes/inode, preferences, supervision, outputs, workspace and cursor restore, with five positive owner checks and independent normal-OFF verification. Monitor ctime/link count are not claimed unchanged. | Physical displays, native enable/disable/rotation, input on a fractionally scaled Displays panel, cable hotplug, suspend/resume, compositor restart, secure-output changes and pacing remain open. All new panel input was on the original 1× output. The initial wrong-dispatch attempt failed before any checks and required separate guarded recovery; it remains failed evidence. The Keep-to-file-identity-capture host-loss interval retains manual recovery. |
| Resources and animation | The recorded resource interval on unmerged `35ed87ed` observes **250.581/252.395 MiB** normal mean/peak RSS and **154.033% of one logical core**. Its separate metadata interval records **32.2245 distinct prepared-revision advances/s** in compositor feedback, while RPC wall-time bounds record **59.7333–60.1290 prepared callbacks/s** before mailbox publication. Profiling restores off with fresh independent verification. | Both phases are host-pressure-ineligible; observed RSS below 300 MiB is not accepted budget evidence, and 60 FPS presentation is not established. No causal gain, bottleneck attribution, decoder-drop, pixel-visibility or physical-scanout claim. Separate geometry-only runs record 37.0500 and later 48.3706 feedback intervals/s; the latter has 0.6585 ms mean successful draw CPU wall time. Neither has a host-pressure sample or attributes the unmeasured gaps. The short successful draw stages exclude GPU, full-loop, callback/dispatch and failed-retry timing. Eligible matched normal/animated and cold/warm evidence remains required. See [the source-specific upload trial](../reports/measurements/lucent-live-upload-35ed-native-trial.json), [shape diagnostic](../reports/measurements/lucent-native-shape-diagnostic.json), [later CPU diagnostic](../reports/measurements/lucent-native-shape-cpu-diagnostic.json) and [presentation analysis](native-presentation-analysis.md). |
| International text | Merged `8e6d074b` implements script shaping, hard-paragraph context across soft wraps, exact row-bound visual source/caret maps, tab separator projection, deterministic fallback and opt-in text-input-v3 composition. Five owned native feature checks pass: visible preedit/caret advance, CJK commit/delete/history, masked-focus retirement, multiline composition and RTL navigation. | The fresh owned native suite passes all five feature and five restoration checks; earlier cleanup failures retain their original results. Third-party IME, broader native pixel/scroll details, delayed clipboard/focus races, physical/cross-output text and broader GUI clipboard coverage remain pending. The later Chromium/Notes test passes both directions with nine interaction and twelve restoration checks on one virtual output. Exact merged `8e6d074b` passes six CI checks, full workspace, six controlled service-protocol filters and 832 Vulkan desktop cases; those checks retain their own scope. |
| Application integration | Accepted main includes fresh input-bound context, explicit file/URI Open and expanded XDG parsing. Earlier local-file `8e` and HTTP `%U` `9b` results keep their original scope. The `4c9a4739` harness on coherent `35ed` passes **twelve singular HTTP `%u` feature and nine restoration checks**: pointer, socket and CLI Open each create exactly one fresh owned HTTP document; the declared Action still creates two local documents. | At this integration base, notification links use contextless `UriPort::open`; `XdgUri` starts the default handler without a fresh activation token. This is a remaining implementation gap, distinct from the tested explicit-application Open path. Default handlers, direct token consumption, external navigation, other applications and broader workflows also need native evidence. Application acceptance does not accept the trial’s performance. [Singular HTTP report](../reports/measurements/lucent-native-http-singular-uri-35ed.json), [local-file report](../reports/measurements/lucent-native-application-intents-d1.json) and [plural HTTP report](../reports/measurements/lucent-native-http-uris-9b.json) retain separate scopes; earlier failures stay unchanged. |
| Real-world validation | Several owned native workflows already pass. The coherent `8e6d074b` also passes 16 checks for two simultaneous compositor outputs, one headless, including real panel input at 1×/2×/1.25×, held-button retirement, same-name recreation and exact restoration. | Physical monitors/cable hotplug, suspend/resume, secure-output changes, physical Bluetooth/audio and other power/radio hardware, additional third-party tray applications/menus, other GUI clipboard applications, rich content and delayed-transfer/focus races need broader testing. One genuine wired-only NetworkManager Applet item/menu/information action passes five interaction, four cleanup and thirteen outside-worker preservation checks on unchanged `8e`. The owned real Chromium/Notes keyboard flow passes nine interaction and twelve restoration checks. The earlier output test establishes no Display transaction or presentation FPS. The later four-flow [two-output Displays run](../reports/measurements/lucent-native-two-output-displays.json) separately covers draft/Apply/Revert/timeout/Keep with an owned headless peer; it does not establish physical or secure-output behavior or pacing. |
| Documentation accuracy | Accepted PR100/main delivery and source-specific PR98–100 results are recorded above. Current summaries distinguish those results from historical pending checkpoints. | Each documentation change must pass its own source/site and delivery gates. Keep later summaries source-specific; historical reports retain their original outcomes and limits. |

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
