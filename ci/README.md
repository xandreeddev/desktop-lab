# Coherent hosted Arch release

`arch_release.py` prepares a coherent archive of all six managed clients and the
29 support files in `arch-release-profile.json`. It also retains the acceptance
examples, standalone Component Lab, exact qualification tools, actual test logs,
package/header/ELF evidence and a freshly rendered software Vulkan catalogue.
The ordinary three-client development bundle remains a separate workflow.

The profile is intentionally unbound. Running the script without options only
prints its plan. `--require-source-pin` validates and prints a full application
commit and tree; `--run` refuses an unbound profile or a non-hosted context before
starting a subprocess. Nothing in this directory deploys or boots a VM.

Before a release run, pin `source_commit` and `source_tree` to the final reviewed
application source. Standard checks on that exact source must pass, including
the controlled protocol suites. Commit the reviewed profile/workflow tools and
push the dedicated `ci/lucent-polish-coherent-release` branch. This push-only
workflow checks out the tools and application separately. Ordinary feature
pushes do not select this release workflow.

The remote container has an explicit init, three-CPU/10-GiB/1024-PID caps, no
additional swap allowance and repeated 4500-MiB effective-memory admission.
Cargo compilation uses one job. The workflow freezes one signed Arch package
transaction and the exact Rust toolchain. It tests the full workspace and all
targets, doctests and the profile's selected software Vulkan cases, recording
actual individually successful test results rather than historical counts.

Fresh catalogue validation uses the pinned source's current checker and
producer fingerprints. Story/variant/palette/density/viewport/time inventory
must match the catalogue committed with that source. Every new PNG is retained
and hash checked. Cross-driver PNG equality is not assumed, and this workflow
does not promote captures to the website or record visual approval.

The successful artifact has exactly two root files: the `.tar.zst` archive and
its JSON receipt. Keep both immutable when downloading for offline review. The
receipt binds the archive, manifest, build evidence, application commit/tree,
profile and actual workflow/run identity. Successful artifacts remain for 30
days; bounded failure diagnostics remain for 14 days.

Use the separate [offline receiver](RECEIVING.md) to verify original downloaded
inputs into a new private review directory. It leaves every native/runtime and
visual acceptance flag false and does not execute archived clients or scripts.

An archive pass covers only its declared hosted qualification. The receipt
does not claim standard-check verification, native activation, target ABI,
visual approval or performance acceptance. Those are separate evidence scopes.
Historical cached import/deploy helpers have old pins and unchanged-support
guards; they are not an installer for this generation. A future reviewed
delivery must include the changed Python helpers and their dependencies as
well as all six clients.

Local fixture verification, without compilation or rendering:

```sh
python3 -m unittest discover -s tests -p test_arch_release.py -v
python3 ci/arch_release.py
```
