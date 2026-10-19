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

The sequence exercises preedit candidate movement without canonical commit,
commit/delete with atomic undo/redo, masked-field deactivation, actual multiline
commit and RTL Home/Right navigation. The fixture contains two real newline
characters. Preedit captures are retained for inspection; image inequality alone
is not called proof of preedit ink because a caret can blink. Candidate movement
and unchanged canonical text are reported separately from those images. Offscreen
render tests establish preedit geometry/pixels independently.

An existing compositor input-method owner causes refusal; this harness never
replaces it. These controlled results do not establish third-party IME behavior,
hardware hotplug, physical output acceptance or secure authentication behavior.
The initial native attempt refused admission before starting either fixture;
its generic subprocess failure did not record which file attribute differed.
Restoration checks passed. It is retained as a failed attempt, not proof that
access time caused the refusal. That attempt paired the 8e SDK examples with the
older a7f desktop, so it is not coherent 8e runtime acceptance. The corrected file
checks pass real temporary file cold-read and mutation-refusal regressions; a
native rerun remains pending.
