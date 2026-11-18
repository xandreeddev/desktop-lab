# Hosted Ubuntu APT sources

The Rust CI dependency step selects Ubuntu's official HTTPS archive and security
endpoints through `scripts/ci-ubuntu-sources.py`. It edits only recognized `URIs`
fields in the current runner's `/etc/apt/sources.list.d/ubuntu.sources`. Suites,
components, signing fields, architectures, other options, comments and unrelated
stanzas remain unchanged. Security-only stanzas use `security.ubuntu.com`;
nonsecurity stanzas use `archive.ubuntu.com`. Mixed suites or ambiguous URI lists
fail validation before the file is written. Already normalized files are valid
no-ops. Disabled stanzas remain unchanged.

The current workflow uses `ubuntu-latest`; the observed Ubuntu 26.04 images use
Deb822 sources. The helper requires that regular file and does not rewrite legacy
one-line sources, third-party source files, signing keys or cloud mirror lists.
No APT trust, validity or TLS checks are disabled.

## Evidence for replacing the old mirror-list assumption

- [Consolidated run 44277550717, Rust job 134709427380](https://github.com/xandreeddev/desktop-lab/actions/runs/44277550717/job/134709427380)
  checked out `4abb20192ff365d343c8c45b594d519fe0214f69` on Ubuntu 26.04.1,
  AWS `us-west-2`, image `20261111.82.6`. On 2026-11-18 the dependency step failed
  with `Unexpected runner mirror layout` before running APT. The log does not
  contain the mirror file's actual contents. Its advertised image tag
  `ubuntu26/20261111.82` and release both returned HTTP 404 during investigation;
  the exact failed mirror bytes are therefore not established.
- [Successful system run 44273633162](https://github.com/xandreeddev/desktop-lab/actions/runs/44273633162)
  used Azure image `20261111.196.1`. Its published
  [image release](https://github.com/actions/runner-images/releases/tag/ubuntu26/20261111.196)
  targets commit `93a4e4e98715026b60fdf86893cf193b4be3bd47`. That release's
  [APT source script](https://github.com/actions/runner-images/blob/ubuntu26/20261111.196/images/ubuntu/scripts/build/configure-apt-sources.sh)
  always writes the three entries assumed by the old workflow: Azure at priority
  1, official archive at 2 and security at 3.
- Official runner-images [commit f2f91839](https://github.com/actions/runner-images/commit/f2f91839f985dcbde42542557c72de8d0c2970a6),
  merged 2026-11-17, adds AWS image support. The
  [pinned source](https://github.com/actions/runner-images/blob/d9c48363ed37529f6ef929845cda83bbdae0ef74/images/ubuntu/scripts/build/configure-apt-sources.sh)
  conditionally selects Azure or `us-east-2.ec2.archive.ubuntu.com` at priority 1;
  without either cloud match it writes only the two official fallback entries.
  This establishes that the old Azure-only layout assumption is unsupported,
  without claiming which exact list appeared on the failed runner.
- Ubuntu's [APT source documentation](https://manpages.ubuntu.com/manpages/resolute/man5/sources.list.5.html)
  defines separate Deb822 URI, suite, component and signing fields, including
  multiline values and disabled stanzas. The helper changes only the URI field.
  APT's [source parser](https://github.com/Debian/apt/blob/main/apt-pkg/sourcelist.cc)
  uses [StringToBool](https://github.com/Debian/apt/blob/main/apt-pkg/contrib/strutl.cc)
  for `Enabled`: the helper preserves its false words and `0`, accepts its true
  words and `1`, and rejects other nonempty values on recognized Ubuntu entries
  before writing instead of guessing their meaning.

## Validation scope

`python3 -m unittest discover -s tests -p test_ci_ubuntu_sources.py -v` tests only
in-memory data and temporary owned source files. Coverage includes archive versus
security routing, known Azure/AWS/direct/mirror references, multiline fields,
comments, embedded signing keys, disabled and unrelated stanzas, preserved file
permissions, idempotence, malformed input without partial writes, spoofed URI
rejection and the actual helper CLI. No host APT operation, local Rust build,
renderer or VM is used. A future hosted dependency step must establish the native
runner outcome; fixture tests alone do not establish package availability.
