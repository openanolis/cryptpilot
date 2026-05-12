#!/bin/bash
# Regenerate the Rust metadata fixture for Go interop tests.
# Run from the repo root: bash verity-go/metadata/gen_fixture.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

echo "=== Generating testfiles ==="
cd "$REPO_ROOT/verity-core"
python3 make_testfiles.py

echo "=== Running Rust format ==="
cd "$REPO_ROOT"
cargo run -p cryptpilot-verity -- format verity-core/testfiles --hash-output - --label env=prod --force

echo "=== Copying fixture ==="
cp verity-core/testfiles/cryptpilot-verity.metadata.fb verity-go/metadata/testdata/rust.metadata.fb
echo "Fixture updated: verity-go/metadata/testdata/rust.metadata.fb"
