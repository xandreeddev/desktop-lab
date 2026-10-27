# Full-demand decoder experiment — prepared, not executed

This is an isolated consumer of the experimental backend at
`1aebe2e3ffa88416791f122d9fbf01bb0f66f91f` (tree
`d6054f2e06380879025026ca2d683dec2d18ef7a`). It does not wire that backend into
Lucent Desktop. The tools branch is deliberately separate from that frozen
source branch, the old `0bec` experiment, and the production `b2` runtime.

The pinned source's [hosted Rust job](https://github.com/xandreeddev/desktop-lab/actions/runs/40420332356/job/121912881080)
actually passed **37 domain, 67 video, and 21 original-observer tests**, including
all eleven new Source/Cover regressions. That run's whole workflow failed its
separate integration job with `Catalog omits a production rendering input`;
the existing Component Lab provenance was not refreshed for the prototype.
No native comparison or memory improvement follows from that Rust pass.

The prior `d38` tools trial actually passed its 13 consumer tests and all
37/67/21 source gates in Arch, then failed its first A-reopen process because
the raw fixture skipped mandatory owned-asset import metadata. Its driver
recorded `complete_bounded_observer_process_required` but discarded the actual
child stdout/exit code. That failed trial and its original artifact digest
remain recorded in `profile.json`; it supplies no accepted matrix observation.

This corrected consumer has **15 authored Rust tests**: thirteen passed in that
prior trial; the two importer tests and corrected consumer are **not compiled
or executed yet**. The Python driver has **21 pure/tempfile tests**, executable without Cargo,
FFmpeg, GPU, network or VM access. `profile.json` keeps exact expected names,
counts, the original source qualification's log digests and official links.
The tools commit, built binary, archive and hosted artifact remain **unbound**
until a separately reviewed, actual owner push runs the workflow.

## One backend, one consumer binary, four fresh measured processes

The existing pinned observer accepts only its known-size FHD → 4K → FHD phases.
This experiment therefore adds a separately identified example consumer rather
than silently changing its CLI or claiming that it covers Source demand.

The driver keeps a pristine checkout of `1a`, takes a disposable `git archive`,
then adds only the explicit `full-demand-observer` example declaration and
these three consumer files: `observer.rs`, `observer_tests.rs`, `accounting.rs`.
It hashes every archived backend file before this change and checks the exact
permitted resulting roster before and after every build stage. The original
dependency lock stays byte-identical. Both backend source and consumer/tools
commit identities are recorded; this derivative is not called an unmodified
`1a` observer. No different backend library, decoder option or allocator policy
is built for either mode.

There are two separate comparisons. Both use the same release binary and
immutable video, encoded once. Every case starts a new process with a distinct
PID. The input has an explicit typed `sequence` and `mode`; no heuristic chooses
a sequence from an observed target.

The same binary first runs an explicit `--prepare` operation in a separate
importer process. It calls the existing `WallpaperPlaybackPort::prepare` once,
letting the backend create the owned source, poster and `asset.json`. The
encoded original lives outside owned storage; the driver never synthesizes
metadata. It verifies the returned descriptor against the actual metadata and
copied immutable source bytes, then freezes the verified metadata and records
all three file hashes. Every case gets that exact same descriptor; changes to
its metadata, poster or source are rejected before each case and at completion.
The importer must finish before all four distinct measured process lifetimes,
and its PID cannot become a measured case. Its decoding/allocations therefore
do not prewarm cold-FHD allocations in A's process. It is not a fifth matrix
observation.

**Matrix A, `known_targets`: cold FHD → 4K → restored FHD.** Its fresh process
admits FHD before its first play, with no source-size/4K phase or previous frame
in that process. This is the comparison for cold-FHD versus warmed-FHD retained
allocations that motivated the desktop investigation.

| Revision | Demand | Expected decoded RGBA extent |
| --- | --- | --- |
| 1 | Cover FHD | 1920 × 1080 |
| 2 | Cover 4K | 3840 × 2160 |
| 3 | Cover FHD | 1920 × 1080 |

**Matrix B, `source_and_targets`: Source → FHD → 4K → FHD → Source.** Its FHD
phases are already warm after a 4K source-size publication. This checks full
target semantics and cannot substitute for Matrix A's cold-FHD comparison.

| Revision | Demand | Expected decoded RGBA extent |
| --- | --- | --- |
| 1 | Source (`Some(demand { target: None })`) | 3840 × 2160 |
| 2 | Cover FHD | 1920 × 1080 |
| 3 | Cover 4K | 3840 × 2160 |
| 4 | Cover FHD | 1920 × 1080 |
| 5 | Source | 3840 × 2160 |

An outer `try_latest() == None` remains a missing/contended observation. It is
not revision 1 or 5's admitted source-size demand. All revisions must advance
exactly once; the backend's separate source tests cover equal-demand no-ops,
stale/same-revision changes, contention, exhaustion, alpha-safe conversion,
held pixel ownership and FFmpeg timing/EOF/cancellation.

In each **reopen control**, the exact old worker must cancel and join before the
next worker starts: three play calls/joins for A, five for B. Each **retarget**
case uses one play call and one worker across its sequence, with one final
cancel/join. Both consumers
retain last-good pixels until replacement and reject mismatched revisions;
phase metric resets do not relabel old storage. The control resets its observed
source clock when a new decoder starts; that reset is not counted as an EOF
wrap. A failed, early, panicked or unjoined worker stops the sequence and
prevents the next process case. No allocator query runs on that failure path.
One API play/worker is observable here; this consumer does not expose native
FFmpeg context pointers. The backend's existing failed-rewind fallback remains
unchanged, so this does not prove that every codec or failed seek can retain
one native decoder context indefinitely.

Each target must publish a matching frame in **less than 5000 ms**, checking a
fresh clock after taking and validating the state lock. The admitted elapsed
time is retained. Each successful phase then records 12 seconds from readiness,
at bounded 250 ms memory sampling intervals. Matching frames must have the
expected actual RGBA byte length, natural source aspect and a normalized PTS
from the actual generated video's ffprobe table (one ns conversion allowance).
First/last PTS, actual phase clocks, loop wraps, stale publications, storage
capacity and process RSS are observations; publication counts are **not FPS**.
For the control, target readiness starts after the old worker's join; its
separate join duration and before-target clock retain that preceding cost.
The fixed case order is A-reopen, A-retarget, B-reopen, B-retarget, once each.
All four process lifetimes must be ordered and nonoverlapping, and each record
must bind the same binary/input hashes. These observations are not a repeated,
randomized performance benchmark.

The final frame is still held at the first after-join allocator snapshot, then
explicitly dropped. There are **exactly three stopped-only mallinfo2 snapshots
per process**: before play, after final exact join with latest held, and after
latest drop. Matrix A ends with FHD storage held; B ends with source-size
storage held. There are twelve allocator snapshots across the four processes.
During-target samples contain no allocator statistics. These
glibc counters describe accounting, including free arenas and direct mappings;
they do not by themselves identify Rust object ownership or resident pages.
No trimming, allocator tuning, preload, cache dropping or host policy change
is permitted.

## Generated input and finite hosted budget

The input recipe is `testsrc2=size=3840x2160:rate=60`, **240 frames**, H.264
`libx264`, `yuv420p`, `veryfast`, CRF 28, GOP/keyint 120, scene-cut disabled,
three B frames and three reference frames, two encoder threads and one filter
thread. The exact command, actual encoder/version output, ffprobe metadata,
source bytes and SHA-256 are retained. This is a reproducible recipe, not a
promise that different FFmpeg/x264 builds produce identical encoded bytes.
It is a generated opaque signal, not a representative wallpaper corpus.

The prepared workflow pins the official Arch image, Rust 1.99.0 and owner-only
branch push. It uses one runner/container with **3 CPU, 10 GiB and no swap**.
The ordinary `checks` workflow excludes only this experiment's branch push so
it does not launch a second unbounded Rust job in parallel; ordinary pushes to
other branches and all pull requests keep their existing checks. This isolated
tools branch is not a replacement for the prototype's failed integration gate.
A new three-sample **4500 MiB** effective-availability admission is mandatory
before build, encoding, preparation and **each** fresh-process case. A single downloaded,
signed Arch transaction is frozen with database, archive and package hashes;
the exact FFmpeg public headers and observer ELF metadata are retained. This
supports provenance, not compatibility with an older VM FFmpeg installation.

The eight Cargo stages are fetch, format, selected-library/example clippy,
37-domain test gate, 67-video gate, 21-original-observer gate, 15-new-consumer
gate, and one release-observer build. There is no desktop release build, GPU
test, deployment, guest baseline or native session in this workflow.

The success upload has **one directory input**:
`full-demand-observer-envelope/`, containing the portable `.tar.zst` and its
receipt. The receipt binds the actual inner archive, experiment record, binary,
input and workflow provenance. The later GitHub upload artifact digest/ID must
also be saved from the actual successful run and checked when downloading;
the action's outer artifact digest and the inner tar digest are different
identities. Never bind an archive merely because its expected filename exists.

Every child attempt saves its bounded JSON input and newest admission before
starting, then its PID, exit code, timeout state, bounded stdout, stderr hash
and fixed failure reason before applying the existing complete-process gate.
A failed child still prevents the next case; retaining diagnostics does not
relax exit, output, native report, target, PTS, join or allocator validation.

## Review and execution boundary

Safe local checks, already used for preparation:

```sh
python3 -m unittest discover -s ci/full-demand-observer -p tests.py -v
python3 ci/full-demand-observer/run.py
```

The default second command prints an inert plan only. Do not run `--run`
locally. No workflow push or dispatch is authorized by this preparation
document. After review, the owner can authorize a normal push of the exact
tools commit to the prepared branch; the driver then accepts only that actual
hosted context and separately checked-out backend SHA.

If it later passes, compare cold-FHD, 4K and restored-FHD RSS within Matrix A,
then the separately labeled Source transitions in B, and the stopped allocator scopes using
the actual records, not only a peak or a count of published frames. Preserve
failed trials. A lower observer RSS is not whole-shell ≤300 MiB acceptance,
VM/physical-display acceptance, source-frame presentation at 60 Hz, or a reason
to skip source-specific production integration and graphical tests.
