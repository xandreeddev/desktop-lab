# Offline coherent release receiver

`arch_receive.py` verifies an explicitly supplied release into a new private
directory. Its default invocation is inert. It has no downloader, installer,
service control, VM connection, build or executable-launch operation.

The trust anchor is a caller-reviewed binding file and its separately supplied
SHA-256. Prepare this binding from the intended local source/tool commits and
authenticated GitHub API responses after the release run and job have completed
successfully. Do not generate expected identities by trusting values found in
the downloaded archive. The offline receiver checks the supplied evidence; it
does not authenticate GitHub metadata by itself.

Retain these original API responses in a metadata directory:

- `run.json`: the exact workflow run attempt.
- `job.json`: its successful `arch-release` job, including `run_attempt`.
- `workflow.json`: the workflow with path `.github/workflows/arch-release.yml`.
- `artifact.json`: the unexpired artifact detail, including GitHub's published
  `sha256:` digest and `size_in_bytes`.

The binding has this shape. Replace every placeholder with an independently
reviewed expected value; `profile_sha256` is the profile committed at the
workflow SHA. Run/job/workflow/artifact IDs and attempt are positive integers.

```json
{
  "schema_version": 1,
  "repository": "xandreeddev/desktop-lab",
  "actor": "xandreeddev",
  "branch": "ci/lucent-polish-coherent-release",
  "workflow_sha": "FULL_REVIEWED_WORKFLOW_COMMIT",
  "source_commit": "FULL_REVIEWED_APPLICATION_COMMIT",
  "source_tree": "FULL_REVIEWED_APPLICATION_TREE",
  "profile_sha256": "REVIEWED_PROFILE_SHA256",
  "run_id": 1,
  "run_attempt": 1,
  "job_id": 1,
  "workflow_id": 1,
  "artifact_id": 1,
  "artifact_bytes": 1,
  "artifact_zip_sha256": "PUBLISHED_ARTIFACT_SHA256_WITHOUT_PREFIX",
  "metadata_sha256": {
    "run.json": "ORIGINAL_RESPONSE_SHA256",
    "job.json": "ORIGINAL_RESPONSE_SHA256",
    "workflow.json": "ORIGINAL_RESPONSE_SHA256",
    "artifact.json": "ORIGINAL_RESPONSE_SHA256"
  }
}
```

Supply the original artifact ZIP and a separate byte-identical copy of its
`artifact-receipt.json`; whitespace normalization of the receipt is rejected.
The source argument is a clean checkout at the expected application commit and
tree. Its Git object database must also contain the reviewed workflow commit so
the receiver can compare archived tools with committed blobs. The local
receiver's trusted catalogue checker must match the checker in that application
commit. Archive Python files are never imported or executed.

```sh
python3 ci/arch_receive.py --verify \
  --archive /absolute/download/artifact.zip \
  --receipt /absolute/download/artifact-receipt.json \
  --binding /absolute/review/binding.json \
  --binding-sha256 REVIEWED_BINDING_SHA256 \
  --metadata /absolute/download/metadata \
  --source /absolute/clean-application-checkout \
  --output /absolute/new-review-directory
```

An optional cached provider snapshot requires both `--providers` and
`--providers-sha256`. It is a separately pinned observation, never evidence of
the target's current installed state. Static inspection invokes only the local
`readelf` against pinned file descriptors. It checks ELF architecture,
interpreter, direct library/version requirements and unreviewed loader-path
overrides. With a reference, it compares direct SONAME/version availability and
FFmpeg public-header hashes. It does not resolve individual symbols, recursive
dependency closure, loader environment or struct layouts; `runtime_ABI_accepted`
always remains false.

The receiver bounds ZIP size to 512 MiB, preflights its two-member directory
before allocating an index, and streams zstd into a tar bounded to 1536 MiB.
Tar headers are checked before extensions can allocate data. Duplicate names,
traversal, links, devices, sparse/PAX/GNU extensions, unexpected members and
undeclared trailers are rejected. It admits at least 4 GiB of free disk for the
bounded original, compressed, expanded and unpacked files. Extracted files stay
nonexecutable with mode `0600` inside a new mode `0700` directory.

Verification covers the exact six managed clients, three acceptance clients,
29 support files, five example sources, standalone Lab, all reviewed notices,
qualification tools, test/resource/package/header/ELF evidence and fresh
catalogue. Source files and tools must match the expected Git blobs. Test logs
must reproduce the declared individually successful results. The catalogue's
current producer fingerprints, complete reviewed inventory and each new PNG
hash are checked without requiring another driver's PNG bytes to match.

`receiver-report.json` records archive verification and static findings. Raw
inputs remain under `originals/`, the two archive envelope files under `packed/`
and reviewable files under `payload/`. Failures retain only this newly created
work directory and write `verification-failed.json`; existing input/output
directories are never overwritten. No automatic promotion follows verification.

Standard checks remain explicitly unverified in this report. Before delivery,
the exact final application source still needs successful evidence for all four
standard jobs, including the controlled protocol suite (49 cases at this
preparation). Human review of the retained visuals and target/native/performance
acceptance also remain separate. A successful receiver report cannot satisfy
those scopes.

Fixture checks use owned files and stub Git/readelf/decompression boundaries
where a complete synthetic release is assembled. The actual notice copier,
catalogue checker and capture validator run against tiny owned fixtures. They do not inspect
a downloaded binary, run a real release build, or contact a desktop:

```sh
python3 -m unittest discover -s tests -p 'test_arch_*.py' -v
```
