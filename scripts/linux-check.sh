#!/usr/bin/env bash
# Runs the Linux checks (formatting, clippy and tests) on a Linux host or in a distrobox.
#
# Needs the system packages, Rust (with the wasm32-unknown-unknown target) and Trunk from
# DEVELOPMENT.md ("Linux").
# Usage: scripts/linux-check.sh
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n==> %s\n' "$*"; }

[ "$#" -eq 0 ] || { echo "usage: scripts/linux-check.sh" >&2; exit 2; }

step "rustfmt"
cargo fmt --all --check

step "trunk build"
(cd app/ui && trunk build)

step "clippy (native)"
cargo clippy --workspace --all-targets -- -D warnings

step "clippy (wasm UI)"
cargo clippy -p atrium-ui --target wasm32-unknown-unknown -- -D warnings

step "tests"
cargo test --workspace --exclude atrium-ui

step "all Linux checks passed"
