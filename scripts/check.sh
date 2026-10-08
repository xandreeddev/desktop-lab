#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
python3 -m unittest discover -s tests -v
shellcheck scripts/*.sh configs/lucid/*.sh
cargo fmt --manifest-path lucent/Cargo.toml --all --check
cargo clippy --manifest-path lucent/Cargo.toml --locked --workspace --all-targets -- -D warnings
cargo test --manifest-path lucent/Cargo.toml --locked --workspace
