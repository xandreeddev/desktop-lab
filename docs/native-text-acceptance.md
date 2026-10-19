# Native international text acceptance

`vm/test-international-text.py` is an opt-in acceptance harness for the independent
international-text consumer. Importing it performs no native actions. It requires
an unlocked, idle 1920×1080 single-output VM at scale1 and explicitly supplied
example/peer paths and SHA256 hashes from the identified coherent release. The
installed and running desktop must match `--desktop-sha256` too. The operator
stages only those owned release files; the harness never replaces an installed
input method or kills its process.

```sh
python3 vm/test-international-text.py \
  --source-commit FULL_RELEASE_COMMIT \
  --desktop-sha256 RELEASE_DESKTOP_SHA256 \
  --example-bin /absolute/owned/staging/international-text \
  --example-sha256 RELEASE_EXAMPLE_SHA256 \
  --peer-bin /absolute/owned/staging/input-method-peer \
  --peer-sha256 RELEASE_PEER_SHA256
```

The desktop and SDK example share `--source-commit`. An independently corrected
acceptance peer may instead supply `--peer-source-commit FULL_FIXTURE_COMMIT`;
it defaults to the common source commit. Both require full lowercase 40-digit
commit IDs. Reports retain the legacy common `source_commit` and explicitly
record `source_commits.desktop`, `.example` and `.peer`, alongside all three
binary hashes. A peer-only build does not relabel the installed desktop/example
or the immutable nine-binary release archive as rebuilt.

Before every native action it checks the original output/workspace, unchanged
owned processes/cgroups and socket peers, closed shell panels, absent credential
requests, no managed applications or foreign layers, safe compositor bindings
and retained pointer authority. Keys require both the exact component field focus
and a fresh active/matching input-method observation. Pointer targets use current
submitted, fully visible hit geometry and a frozen native layer lifetime.

The example and non-replacing peer run in distinct random transient systemd
units. Typed `ExecStart` arguments, invocation identity, executable and process
birth/cgroup are authority; command substrings and namespaces alone are not.
Both names must be absent by typed systemd inspection before startup. A refused
or lost start reply, or a missing captured invocation, never authorizes stopping a
lookalike existing unit; uncertain launches remain explicit failures and any
actually created fixture unit keeps its independent watchdog.
Both units expire after210seconds independently of the host harness, with a
10-second stop timeout. Input admission ends after170seconds. Cleanup retires the
exact example before the peer, removes only its captured socket inode, and
restores the cursor only while the original idle session and pointer authority
remain valid. A host crash does not automatically restore the cursor. Private
checkpoints, captures and reports remain under ignored `reports/local` on failure.

Binary probing and settings checkpoints pin a bounded regular-file descriptor
and compare device, inode, mode, owner, group, size, modification time and change
time against both the descriptor and pathname before/after reading. Access time
is excluded because a cold read can update it under relatime. Content hashes,
size bounds and refusal of replacement/symlinks remain required. Subprocess
failures identify the fixed probe/checkpoint step and retain only exit metadata,
stderr length/hash and allowlisted fixed reasons, without process payload.

The sequence exercises preedit ink and caret movement without canonical commit,
commit/delete with atomic undo/redo, masked-field deactivation, actual multiline
commit and RTL Home/Right navigation. The fixture contains two real newline
characters. Preedit captures are retained for inspection; image inequality alone
is not proof of preedit ink because a caret can blink. The bounded mapped-popup
proof below separately checks movement, new noncaret ink and unchanged canonical
text. Offscreen render tests establish preedit geometry/pixels independently.

An existing compositor input-method owner causes refusal; this harness never
replaces it. These controlled results do not establish third-party IME behavior,
hardware hotplug, physical output acceptance or secure authentication behavior.
The initial native attempt refused admission before starting either fixture;
its generic subprocess failure did not record which file attribute differed.
Restoration checks passed. It is retained as a failed attempt, not proof that
access time caused the refusal. That attempt paired the 8e SDK examples with the
older a7f desktop, so it is not coherent 8e runtime acceptance. The corrected file
checks pass real temporary file cold-read and mutation-refusal regressions.

A later attempt used the coherent 8e desktop, SDK example and original peer.
It failed before native input: the peer panicked on the legitimate initial seat
Capabilities event because the eventless `delegate_noop!` form is unreachable on
events. The failed report retains its cleanup/guard failures; an independent
read-only check at 2026-10-19 20:39:54 UTC subsequently verified the peer unit was
failed with no processes, no example had been created, and exact settings,
wallpaper, output, workspace, pointer and idle/unlocked 8e desktop were preserved.
That separate observation does not turn the failed attempt into a pass.

The acceptance-peer-only correction explicitly ignores legitimate seat/surface
events while retaining static-seat, registry-retirement and nonreplacement
guards. Its independently compiled fixture source is
`b4d67a4560948b78549a371d3ef00ef06089f4f1`; binary SHA256
`ff22d164e475893f12d45c34cdcdd82ceeb8b92c22a4ec7c720bf184762c016f`
(894,008 bytes). Formatting and the peer-only build passed. The desktop/example
remain at `8e6d074bfcb30a0bcc0b371afddcd50c08993af0`, and the original archive
remains unchanged. At that point the native rerun was pending; the subsequent
failed attempt is recorded below. Compilation alone establishes no successful
IME interaction.

The subsequent b4d-peer native attempt no longer panicked: both fixtures were
admitted and the owned ordinary field was focused/cleared, but it failed at the
initial candidate rectangle. The failed report's cleanup flags remain false.
A later independent diagnostic found both units absent with no processes; it
also recorded the pointer at the last owned position (960,138), instead of the
original (960,540). It did not prove why strict retirement failed or make the
original cleanup pass. A stale example socket also blocked the guarded
restoration path. The earlier failed reports and independent observations remain
separate evidence; no SDK defect is inferred from this fixture failure.

The exact deployed Hyprland v0.56.2 source at
`efb50993780079460b0cbed1363e2166a2de1d9f` requires a committed texture before
mapping this popup, and only mapped popups update their boxes. The previous peer
never attached a buffer. Its rectangle event is popup-local, so increasing the
event's x coordinate is not a valid general test of caret motion. See the pinned
[popup positioning](https://github.com/hyprwm/Hyprland/blob/efb50993780079460b0cbed1363e2166a2de1d9f/src/managers/input/InputMethodPopup.cpp#L74)
and [buffer mapping](https://github.com/hyprwm/Hyprland/blob/efb50993780079460b0cbed1363e2166a2de1d9f/src/protocols/InputMethodV2.cpp#L96)
implementations.

The corrected peer commits one immutable 8×8 opaque barcode from a 256-byte
anonymous shared-memory file only while a genuine ordinary input method is
active. Its empty input region accepts no pointer input; it never grabs a
keyboard. On deactivation it detaches the buffer. Bounded PPM captures cover
the owned example's surface bounds. The host requires one unique barcode in each capture,
combines its observed position with the popup-local hint to reconstruct both
global caret rectangles, and requires caret advancement after preedit. It also
requires newly painted foreground ink between excluded old/new caret strips,
outside either barcode, with an unchanged dominant field background. A blinking
caret, reused image, missing/duplicate marker, background change, wrong-region
ink or absent motion fails the pure negative fixtures. This is controlled native
preedit evidence when executed; it does not establish third-party IME behavior.
The mapped peer subsequently compiled and passed source review at independent
fixture source `3beda6172b8ce7eeb7be77b7ebd055511388f6de`, corresponding to native
fixture code `45e4f8a9ad68593a27edbf6ad065b37f3aec709f`. Its SHA256 is
`5e823533c49713b1c5f488bd51564cf84838cd3df6e39384ac276749c53d93cc`
(934,024 bytes). The deployed desktop and SDK example remain at coherent `8e`;
that peer-only build changes neither the production runtime nor the original
nine-binary archive. Mapping code that compiled is not proof that a mapped
popup or preedit ink was observed in the compositor.

Cleanup now records a bounded trace of original unit/process observation phases,
fixed exception classes and allowlisted D-Bus error names. It retains protected
metadata and comparisons against pinned identities, without actual argv,
credential/text payloads or raw exception messages. Subprocess failure metadata
also remains attached to its fixed restoration phase. Failed identity checks
still block socket cleanup and pointer restoration. The first attempts provided
no trace proving why strict retirement failed. The later mapped-peer attempt
records a concrete acknowledged-stop transition, described below; a separate
correction addresses only that observed lifecycle state.


## Latest mapped-peer attempt

The fresh attempt `international-text-native-5231c104b8c1b3a738b1f268` used the
`8e6d074b` desktop/example and independently built `3beda617` mapped peer. It
failed at `exact_owned_pointer_arrival`, before composition checks. Its `checks`
object is empty: **zero native composition checks passed**. It establishes no
successful preedit, commit, delete, multiline or RTL interaction, and proves no
SDK defect. The original failed report and its false retirement/restoration
flags remain unchanged.

Its bounded cleanup trace records the original owned unit before an acknowledged
`StopUnit`, followed by `deactivating` with zero MainPID, cleared invocation and
cgroup, and `Transient=false`; a later observation finds that unit absent. The
strict completion check rejected that intermediate cleared-metadata state.
This actual trace supports the narrow shared helper correction `47fa3ac96d7`.
The correction still requires the exact captured invocation/process at stop
admission and a successful stop acknowledgement. It accepts only the matching
cleared stopping/terminal shape, then independently proves that the captured
PID birth and recursive cgroup have drained. A cleared unit is never initial
stop authority; changed argv or a same-name successor remains refused, and a
lost stop reply does not authorize this completion path. All 26 focused ownership
checks pass. Native verification of the corrected cleanup path is pending.

Separate independent receipts in the same private attempt directory establish
recovery without changing the failed result:

- `independent-readonly-after-mapped-failure.json` finds both owned units and
  processes absent, the same original `8e` desktop and original cursor position.
- `independent-guarded-socket-retirement.json` verifies that only the captured
  stale socket was retired after connection refusal, with the same desktop,
  original cursor and unlocked session retained.

These observations verify their stated recovery scope. They do not rerun the
composition sequence or turn it into native IME acceptance. A fresh guarded run
is still needed for actual popup/caret/preedit ink, commit/delete/undo,
multiline, masked-field deactivation and RTL navigation. Third-party IMEs and
secure authentication remain separate acceptance; no presentation/FPS or
resource result follows from this attempt.
