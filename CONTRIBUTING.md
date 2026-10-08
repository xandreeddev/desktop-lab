# Contributing

Run `scripts/check.sh` with Python 3.11+, ShellCheck and a current Rust toolchain. Linux Rust builds require Wayland, libxkbcommon and pkg-config development files. CI checks configuration/recovery code and the Rust workspace; it does not claim to run graphical VM tests.

Test shell changes in their own clone. Keep credentials, full inventories, qcow2 disks and raw logs out of Git. Pin upstream revisions and checksums before updating installers. Review installer changes, validate the installed configuration schema, and test rollback before replacing login-time startup. Never edit packaged Omarchy files.

Record actual versions, test commands and observations. Distinguish process RSS from cgroup usage and GPU allocations. Screenshots alone do not prove that a control works. New Lucent features need a working vertical slice before introducing a new crate or abstraction. Preserve the native desktop/framework boundary and keep rendering independent of OS service operations.
