# Contributing

This public repository currently accepts changes only from `@xandreeddev`, its
sole owner and sole account with write access. Do not add collaborators or write
credentials without revisiting this policy.

## Merging and publishing

Create a branch and a pull request targeting `main`. The branch must be current
with `main`, all review conversations must be resolved, and GitHub Actions must
pass `site`, `integration-code` and `rust`. These rules apply to the owner too;
direct pushes, force pushes and deletion of `main` are blocked. An additional
reviewer's approval is not required because the sole maintainer cannot approve
their own PR. The owner chooses when to merge; CI does not merge PRs.

Merging triggers the Pages workflow, which builds and validates the site before
publishing it. Manual Pages runs are available for recovery on `main` only.

The protection settings are versioned in `.github/main-protection.json`. To
reapply them as the authenticated owner from the repository root:

```sh
gh api --method PUT repos/xandreeddev/desktop-lab/branches/main/protection \
  --input .github/main-protection.json
```

GitHub's collaborators-only interaction limit prevents outsiders from opening
PRs, issues or comments. It expires after six months and must be renewed; branch
protection and owner-only write access do not expire. Check the current expiry
and renew with:

```sh
gh api repos/xandreeddev/desktop-lab/interaction-limits
gh api --method PUT repos/xandreeddev/desktop-lab/interaction-limits \
  -f limit=collaborators_only -f expiry=six_months
```

This is a repository-level setting, not a workflow token or scheduled job.
See [GitHub's interaction-limit documentation](https://docs.github.com/en/communities/moderating-comments-and-conversations/limiting-interactions-in-your-repository).

## Development checks

Run `scripts/check.sh` with Python 3.11+, ShellCheck and a current Rust toolchain. Linux Rust builds require Wayland, libxkbcommon and pkg-config development files. CI checks configuration/recovery code and the Rust workspace; it does not claim to run graphical VM tests.

Test shell changes in their own clone. Keep credentials, full inventories, qcow2 disks and raw logs out of Git. Pin upstream revisions and checksums before updating installers. Review installer changes, validate the installed configuration schema, and test rollback before replacing login-time startup. Never edit packaged Omarchy files.

Record actual versions, test commands and observations. Distinguish process RSS from cgroup usage and GPU allocations. Screenshots alone do not prove that a control works. New Lucent features need a working vertical slice before introducing a new crate or abstraction. Preserve the native desktop/framework boundary and keep rendering independent of OS service operations.
