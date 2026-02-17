#!/bin/bash
# Upstream Security Audit
# Compares Ouro security-related changes against the Conti fork.
# Run after each merge to check for unported security fixes.
#
# Usage: bash scripts/upstream_security_audit.sh [last_sync_commit]

set -euo pipefail

UPSTREAM_DIR="/tmp/ouro"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OURO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Refresh or clone upstream
if [ -d "$UPSTREAM_DIR/.git" ]; then
    echo ">>> Updating upstream Ouro..."
    git -C "$UPSTREAM_DIR" fetch origin --quiet
    git -C "$UPSTREAM_DIR" reset --hard origin/main --quiet
else
    echo ">>> Cloning upstream Ouro..."
    git clone --quiet https://github.com/parcadei/ouro.git "$UPSTREAM_DIR"
fi

echo ""
echo "=== UPSTREAM SECURITY COMMITS (last 30 days) ==="
echo ""
git -C "$UPSTREAM_DIR" log --oneline --since="30 days ago" \
    --grep='guard\|security\|fuzz\|leak\|overflow\|limit\|DoS\|depth\|crash\|panic\|sanitize\|unsafe\|CVE' \
    -i || echo "(none found)"

echo ""
echo "=== SECURITY-CRITICAL FILE DIFFS ==="
echo ""

SECURITY_FILES=(
    "crates/ouro/src/resource.rs"
    "crates/ouro/src/heap.rs"
    "crates/ouro/src/parse.rs"
    "crates/ouro/src/value.rs"
    "crates/ouro/src/object.rs"
    "crates/ouro/src/exception_private.rs"
    "crates/ouro/src/types/py_trait.rs"
)

for file in "${SECURITY_FILES[@]}"; do
    upstream_file="$UPSTREAM_DIR/$file"
    our_file="$OURO_DIR/$file"

    if [ ! -f "$upstream_file" ]; then
        continue
    fi
    if [ ! -f "$our_file" ]; then
        echo "WARNING: $file exists upstream but NOT in our fork"
        continue
    fi

    # Count lines that mention security-related terms
    security_diff=$(diff "$upstream_file" "$our_file" 2>/dev/null \
        | grep -ciE 'guard|depth|limit|leak|overflow|panic|unsafe|sanitize' || true)

    if [ "$security_diff" -gt 0 ] 2>/dev/null; then
        echo "--- $file: $security_diff security-related diff lines ---"
        diff "$upstream_file" "$our_file" 2>/dev/null \
            | grep -iE 'guard|depth|limit|leak|overflow|panic|unsafe|sanitize' \
            | head -10
        echo ""
    fi
done

echo ""
echo "=== FEATURES PRESENT UPSTREAM, ABSENT IN FORK ==="
echo ""

# Check for DepthGuard
upstream_dg=$(grep -rc "DepthGuard" "$UPSTREAM_DIR/crates/ouro/src/" 2>/dev/null || echo "0")
our_dg=$(grep -rc "DepthGuard" "$OURO_DIR/crates/ouro/src/" 2>/dev/null || echo "0")
echo "DepthGuard usage:  upstream=$upstream_dg  ours=$our_dg"

# Check for fuzz targets
upstream_fuzz=$(ls "$UPSTREAM_DIR/crates/fuzz/fuzz_targets/"*.rs 2>/dev/null | wc -l | tr -d ' ')
our_fuzz=$(ls "$OURO_DIR/crates/fuzz/fuzz_targets/"*.rs 2>/dev/null | wc -l | tr -d ' ')
echo "Fuzz targets:      upstream=$upstream_fuzz  ours=$our_fuzz"

# Check for HeapGuard
upstream_hg=$(grep -rc "HeapGuard" "$UPSTREAM_DIR/crates/ouro/src/" 2>/dev/null || echo "0")
our_hg=$(grep -rc "HeapGuard" "$OURO_DIR/crates/ouro/src/" 2>/dev/null || echo "0")
echo "HeapGuard usage:   upstream=$upstream_hg  ours=$our_hg"

echo ""
echo "=== SECURITY TEST SUITE STATUS ==="
echo ""
security_tests=$(ls "$OURO_DIR/crates/ouro/test_cases/security__"*.py 2>/dev/null | wc -l | tr -d ' ')
echo "Security test files: $security_tests"
for f in "$OURO_DIR/crates/ouro/test_cases/security__"*.py; do
    if [ -f "$f" ]; then
        asserts=$(grep -c '^assert ' "$f" 2>/dev/null || echo "0")
        echo "  $(basename "$f"): $asserts assertions"
    fi
done

echo ""
echo "=== DONE ==="
