#!/usr/bin/env bash
# Run all tests for ticket-rs.
# Usage: ./test.sh              # fast, skip integration tests
#        ./test.sh --all        # full suite (unit + integration)
#        ./test.sh --watch      # run on file changes (requires cargo-watch)
set -euo pipefail

cd "$(dirname "$0")"

MODE="${1:-quick}"

echo "=== ticket-rs test suite ==="
echo ""

case "$MODE" in
    --all|-a|full)
        echo "--- Unit tests (bin) ---"
        cargo test --bin ticket-rs 2>&1
        echo ""
        echo "--- Integration tests ---"
        cargo test --test integration 2>&1
        echo ""
        echo "--- Clippy ---"
        cargo clippy -- -D warnings 2>&1 || true
        ;;
    --watch|-w)
        if command -v cargo-watch &>/dev/null; then
            cargo watch -x "test --lib" -x "test --test integration"
        else
            echo "cargo-watch not installed. Install with: cargo install cargo-watch"
            exit 1
        fi
        ;;
    quick|*)
        echo "--- Unit tests (quick) ---"
        echo "Run ./test.sh --all for full suite (unit + integration + clippy)"
        echo ""
        cargo test --bin ticket-rs 2>&1
        ;;
esac

echo ""
echo "=== Done ==="