#!/usr/bin/env bash
#
# Publish every crate in the workspace to crates.io in dependency order.
# Cargo requires upstream crates to exist on the index before downstream
# crates can be published, so this drives the sequence and waits for
# index propagation between steps.
#
# Usage:
#   scripts/publish.sh              # real publish (requires `cargo login`)
#   scripts/publish.sh --dry-run    # dry-run each step; skips crates whose
#                                   # internal deps aren't yet on crates.io
#                                   # (expected to fail after jc-core / jc-adf)
#
# Prereqs:
#   - `cargo login <token>` completed
#   - Working tree clean, tagged as v<version>
#   - CHANGELOG.md updated
#
set -euo pipefail

DRY_RUN=""
if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN="--dry-run"
fi

cd "$(dirname "$0")/.."

# Leaf crates first — no internal deps, can publish immediately.
echo ">>> Publishing jc-core"
cargo publish $DRY_RUN -p jc-core
echo ">>> Publishing jc-adf"
cargo publish $DRY_RUN -p jc-adf

if [[ -z "$DRY_RUN" ]]; then
    # Wait for crates.io index propagation before the downstream crates
    # try to resolve their freshly-published upstream.
    echo ">>> Waiting 30s for index propagation"
    sleep 30
fi

# Product clients depend on jc-core + jc-adf.
echo ">>> Publishing jc-jira"
cargo publish $DRY_RUN -p jc-jira
echo ">>> Publishing jc-conf"
cargo publish $DRY_RUN -p jc-conf

if [[ -z "$DRY_RUN" ]]; then
    echo ">>> Waiting 30s for index propagation"
    sleep 30
fi

# Binary crate depends on all four.
echo ">>> Publishing jc"
cargo publish $DRY_RUN -p jc

echo ">>> Done."
