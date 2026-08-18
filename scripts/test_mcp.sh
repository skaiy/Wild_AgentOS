#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
PASS=0
FAIL=0

green() { printf '\033[32m%s\033[0m\n' "$1"; }
red()   { printf '\033[31m%s\033[0m\n' "$1"; }
bold()  { printf '\033[1m%s\033[0m\n' "$1"; }

header() {
    bold "━━━ $1 ━━━"
}

check() {
    if [ $? -eq 0 ]; then
        green "  ✓ $1"
        PASS=$((PASS + 1))
    else
        red "  ✗ $1"
        FAIL=$((FAIL + 1))
    fi
}

# ── Build ──────────────────────────────────────────────────────────
header "Build"

cd "$PROJECT_DIR"

cargo check 2>/dev/null
check "Library crate compiles"

cd "$PROJECT_DIR/apps/wild_code"
cargo check 2>/dev/null
check "wildcode app compiles"

# ── Unit Tests (MCP related) ──────────────────────────────────────
header "Unit Tests"

cd "$PROJECT_DIR"
cargo test --lib tools::mcp_client::tests --quiet 2>/dev/null
check "mcp_client unit tests"

cargo test --lib tools::mcp::tests --quiet 2>/dev/null
check "mcp unit tests"

# ── Integration Tests ─────────────────────────────────────────────
header "Integration Tests"

cargo test --test mcp_integration_test --quiet 2>/dev/null
check "MCP integration tests (all 9 tests)"

# ── Full lib test suite (quick check) ─────────────────────────────
header "Regression Check"

cargo test --lib --quiet 2>/dev/null
check "Full library test suite"

# ── Summary ───────────────────────────────────────────────────────
echo ""
bold "═══════════════════════════════════════════"
bold "  Results: $PASS passed, $FAIL failed"
bold "═══════════════════════════════════════════"

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
