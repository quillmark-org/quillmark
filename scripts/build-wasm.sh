#!/bin/bash
set -e

# Profile selection. Default is the size-optimized release build used for
# npm publish. `--ci` switches to a fast-compiling profile for PR validation
# where only correctness matters. Keep these two paths in sync with the
# cache namespacing in .github/workflows/{ci,release}.yml.
PROFILE="wasm-release"
MODE_LABEL="release (size-optimized)"
for arg in "$@"; do
    case "$arg" in
        --ci)
            PROFILE="wasm-ci"
            MODE_LABEL="ci (fast compile, unoptimized)"
            ;;
        *)
            echo "Unknown argument: $arg" >&2
            echo "Usage: $0 [--ci]" >&2
            exit 2
            ;;
    esac
done

echo "Building WASM module for @quillmark/wasm... [profile: $MODE_LABEL]"

cd "$(dirname "$0")/.."

# Check for required tools
if ! command -v wasm-bindgen &> /dev/null; then
    echo "wasm-bindgen not found. Install it with:"
    echo "  cargo install wasm-bindgen-cli --version 0.2.118"
    exit 1
fi

echo ""
echo "Building for target: bundler"

# Step 1: Build WASM binary with cargo
echo "Building WASM binary..."
cargo build \
    --target wasm32-unknown-unknown \
    --profile "$PROFILE" \
    --manifest-path crates/bindings/wasm/Cargo.toml

# Step 2: Generate JS bindings with wasm-bindgen
#
# `--weak-refs` opts into FinalizationRegistry-based auto-free for
# wasm-bindgen handles. `.free()` is still emitted as an eager hook for
# callers that want deterministic teardown; opting in just ensures dropped
# handles eventually get reclaimed without manual `.free()` discipline.
# Requires Node 14.6+ / all current evergreen browsers.
echo "Generating JS bindings for bundler..."
mkdir -p pkg/bundler
wasm-bindgen \
    "target/wasm32-unknown-unknown/$PROFILE/quillmark_wasm.wasm" \
    --out-dir pkg/bundler \
    --out-name wasm \
    --target bundler \
    --weak-refs

# Note: a wasm-opt -Oz pass was tried and removed. With the current
# `wasm-release` profile (opt-level=z, fat LTO, codegen-units=1,
# panic=abort, strip=true) it saves only ~15 KB raw / ~10 KB gzipped
# (<0.1%) — not worth the build dependency or the extra build time.

# Step 3: Extract version from Cargo.toml
VERSION=$(cargo metadata --format-version=1 --no-deps | jq -r '.packages[] | select(.name == "quillmark-wasm") | .version')

# Step 4: Create package.json from template
echo "Creating package.json..."
sed "s/VERSION_PLACEHOLDER/$VERSION/" crates/bindings/wasm/package.template.json > pkg/package.json

# Step 5: Copy README and LICENSE files
if [ -f "crates/bindings/wasm/README.md" ]; then
    cp crates/bindings/wasm/README.md pkg/
fi

if [ -f "LICENSE-MIT" ]; then
    cp LICENSE-MIT pkg/
fi

if [ -f "LICENSE-APACHE" ]; then
    cp LICENSE-APACHE pkg/
fi

# Step 6: Create .gitignore for pkg directory
cat > pkg/.gitignore << EOF
*
!.gitignore
EOF

echo ""
echo "WASM build complete!"
echo "Output directory: pkg/"
echo "Package version: $VERSION"

# Show sizes (raw, gzip, brotli — transport size is what matters for delivery).
report_size() {
    local label="$1" file="$2"
    [ -f "$file" ] || return 0
    local raw gz br
    raw=$(du -h "$file" | cut -f1)
    gz=$(gzip -9 -c "$file" 2>/dev/null | wc -c | awk '{printf "%.1fM", $1/1048576}')
    if command -v brotli &> /dev/null; then
        br=$(brotli -9 -c "$file" 2>/dev/null | wc -c | awk '{printf "%.1fM", $1/1048576}')
        echo "WASM size ($label): raw=$raw gzip=$gz brotli=$br"
    else
        echo "WASM size ($label): raw=$raw gzip=$gz"
    fi
}
report_size "bundler" pkg/bundler/wasm_bg.wasm