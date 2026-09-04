#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of walnut-rs, a derivative work of Walnut (GPLv3, Mousavi et al.).
#
# Build the release `walnut-rs` binary — the analog of Walnut's own build.sh, for
# a consumer (e.g. ct-research) that vendors this repo as a submodule and runs
# `bin/walnut-rs`. The release profile enables fat LTO + codegen-units=1 (see the
# workspace Cargo.toml), so a from-scratch build is slow but the resulting engine
# is the fast one the perf campaign measured. Build once; the launcher reuses it.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_DIR"

echo "Building walnut-rs (release)..." >&2
cargo build --release -p wr-cli --bin walnut-rs

echo "Built: $REPO_DIR/target/release/walnut-rs" >&2
