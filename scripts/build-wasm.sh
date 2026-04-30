#!/usr/bin/env bash
# Build the edit-wasm crate and place the output in web/pkg/.
#
# Usage:
#   ./scripts/build-wasm.sh           # release build (default)
#   ./scripts/build-wasm.sh --dev     # debug build (faster compile, larger .wasm)
#
# Prerequisites installed automatically if missing:
#   - wasm-pack  (via `cargo install wasm-pack`)
#   - wasm32-unknown-unknown target (via rustup, declared in rust-toolchain.toml)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# ── Parse arguments ────────────────────────────────────────────────────────────
PROFILE_FLAG="--release"
for arg in "$@"; do
    case "$arg" in
        --dev|-d) PROFILE_FLAG="" ;;
        --release|-r) PROFILE_FLAG="--release" ;;
        *)
            echo "Unknown argument: $arg"
            echo "Usage: $0 [--release|--dev]"
            exit 1
            ;;
    esac
done

# ── Ensure wasm-pack is available ──────────────────────────────────────────────
if ! command -v wasm-pack &>/dev/null; then
    echo "wasm-pack not found – installing via cargo install wasm-pack..."
    cargo install wasm-pack
fi

# ── Build ──────────────────────────────────────────────────────────────────────
echo "Building edit-wasm (profile: ${PROFILE_FLAG:---dev})..."

cd "$REPO_ROOT"

# shellcheck disable=SC2086
wasm-pack build crates/edit-wasm \
    $PROFILE_FLAG \
    --target web \
    --out-dir ../../web/pkg \
    -- --no-default-features

echo ""
echo "Done. Output written to web/pkg/"
echo "Serve with:  python3 -m http.server --directory web/ 8080"
