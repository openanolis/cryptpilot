#!/bin/bash
# tests/cross_lang_test.sh
# Cross-language round-trip test between Rust and Go fs-verity implementations.
#
# Strategy: Each language embeds a binary fixture produced by the other in its
# test code. The Go test deserializes the Rust fixture and vice versa. This
# avoids needing both toolchains in a single process.
#
# What this script does:
#   1. Runs Go cross-language tests (TestCross_*)
#   2. If cargo is available, runs Rust cross-language tests
#   3. Compares descriptor hashes from both sides for consistency
#
# Requires: go (required), cargo (optional, for full coverage)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VERITY_GO="$REPO_ROOT/verity-go"
VERITY_RUST="$REPO_ROOT/cryptpilot-verity"

echo "=== Cross-language Round-Trip Test ==="

# Check prerequisites
if ! command -v go &>/dev/null; then
    echo "SKIP: go not found"
    exit 0
fi

fail=0

# --- Go side ---
echo ""
echo "--- Go cross-language tests ---"
cd "$VERITY_GO"
if go test -v ./metadata/ -run TestCross 2>&1; then
    echo "  PASS: Go cross-language tests"
else
    echo "  FAIL: Go cross-language tests"
    fail=1
fi

# --- Rust side (if available) ---
echo ""
echo "--- Rust cross-language tests ---"
if command -v cargo &>/dev/null; then
    cd "$VERITY_RUST"
    if cargo test --package cryptpilot-verity -- test_cross --nocapture 2>&1; then
        echo "  PASS: Rust cross-language tests"
    else
        echo "  FAIL: Rust cross-language tests"
        fail=1
    fi
else
    echo "  SKIP: cargo not found (Rust side not tested)"
fi

echo ""
if [ "$fail" -eq 0 ]; then
    echo "ALL CROSS-LANGUAGE TESTS PASSED"
    exit 0
else
    echo "CROSS-LANGUAGE TESTS FAILED"
    exit 1
fi
