#!/bin/bash
set -e

# Builds TWO wasm artifacts from the one crate (see
# prose/proposals/wasm-bindings-split.md):
#
#   pkg/core/    — no Typst: parse / load / validate / schema / seed / blueprint
#   pkg/render/  — Typst-backed engine + RenderSession + canvas (API superset)
#
# shipped as one npm package with subpath exports `@quillmark/wasm/core` and
# `@quillmark/wasm/render` (the root `.` export is render, the superset).
#
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

echo "Building WASM modules for @quillmark/wasm... [profile: $MODE_LABEL]"

cd "$(dirname "$0")/.."

# Check for required tools
if ! command -v wasm-bindgen &> /dev/null; then
    echo "wasm-bindgen not found. Install it with:"
    echo "  cargo install wasm-bindgen-cli --version 0.2.118"
    exit 1
fi

# Build one variant: cargo build with the given feature flags, then run
# wasm-bindgen into pkg/<subdir>/. Both variants emit the same wasm artifact
# name (quillmark_wasm.wasm) to the same target path, so they must run
# sequentially — each wasm-bindgen pass consumes the build before the next
# cargo build overwrites it.
#
# `--weak-refs` opts into FinalizationRegistry-based auto-free for
# wasm-bindgen handles. `.free()` is still emitted for callers that want
# deterministic teardown. Requires Node 14.6+ / all current evergreen browsers.
build_variant() {
    local subdir="$1"; shift
    local cargo_feature_args=("$@")

    echo ""
    echo "Building variant: $subdir"
    cargo build \
        --target wasm32-unknown-unknown \
        --profile "$PROFILE" \
        --manifest-path crates/bindings/wasm/Cargo.toml \
        "${cargo_feature_args[@]}"

    mkdir -p "pkg/$subdir"
    wasm-bindgen \
        "target/wasm32-unknown-unknown/$PROFILE/quillmark_wasm.wasm" \
        --out-dir "pkg/$subdir" \
        --out-name wasm \
        --target bundler \
        --weak-refs
}

# render = default features (Typst). core = no features (Typst excluded).
build_variant render
build_variant core --no-default-features

# Note: a wasm-opt -Oz pass was tried and removed. With the current
# `wasm-release` profile (opt-level=z, fat LTO, codegen-units=1,
# panic=abort, strip=true) it saves only ~15 KB raw / ~10 KB gzipped
# (<0.1%) — not worth the build dependency or the extra build time.

# Extract version and create package.json from template
VERSION=$(cargo metadata --format-version=1 --no-deps | jq -r '.packages[] | select(.name == "quillmark-wasm") | .version')
echo ""
echo "Creating package.json..."
sed "s/VERSION_PLACEHOLDER/$VERSION/" crates/bindings/wasm/package.template.json > pkg/package.json

# Copy README, CHANGELOG, and LICENSE files
if [ -f "crates/bindings/wasm/README.md" ]; then
    cp crates/bindings/wasm/README.md pkg/
fi
# Ship the workspace changelog so npmjs renders a Changelog tab for the
# published package (it is listed in package.template.json's "files").
if [ -f "CHANGELOG.md" ]; then
    cp CHANGELOG.md pkg/
fi
if [ -f "LICENSE-MIT" ]; then
    cp LICENSE-MIT pkg/
fi
if [ -f "LICENSE-APACHE" ]; then
    cp LICENSE-APACHE pkg/
fi

# .gitignore for pkg directory
cat > pkg/.gitignore << EOF
*
!.gitignore
EOF

echo ""
echo "WASM build complete!"
echo "Output directory: pkg/  (core/ + render/)"
echo "Package version: $VERSION"

# Show sizes (raw, gzip, brotli — transport size is what matters for delivery).
report_size() {
    local label="$1" file="$2"
    [ -f "$file" ] || return 0
    local raw gz br
    raw=$(du -h "$file" | cut -f1)
    gz=$(gzip -9 -c "$file" 2>/dev/null | wc -c | awk '{printf "%.2fM", $1/1048576}')
    if command -v brotli &> /dev/null; then
        br=$(brotli -9 -c "$file" 2>/dev/null | wc -c | awk '{printf "%.2fM", $1/1048576}')
        echo "WASM size ($label): raw=$raw gzip=$gz brotli=$br"
    else
        echo "WASM size ($label): raw=$raw gzip=$gz"
    fi
}
report_size "core"   pkg/core/wasm_bg.wasm
report_size "render" pkg/render/wasm_bg.wasm

# Size budget on the core artifact: the whole point of the split is that core
# excludes Typst (~8 MB gzip). If core gzip ever crosses this floor, Typst (or
# something nearly as heavy) has crept back into the no-features build — fail
# the build so it can't ship silently. Measured core is ~0.34 MB gzip.
#
# Only enforced for the size-optimized release profile: the `wasm-ci` profile
# is unoptimized, so its absolute size is meaningless against this floor.
# Release is where the artifact actually publishes, so that is where the floor
# matters; CI's structural guarantee is the Cargo feature graph (Typst is not a
# dependency of the no-features build at all).
CORE_MAX_GZIP_BYTES=${CORE_MAX_GZIP_BYTES:-700000}
if [ -f pkg/core/wasm_bg.wasm ] && [ "$PROFILE" = "wasm-release" ]; then
    core_gz_bytes=$(gzip -9 -c pkg/core/wasm_bg.wasm | wc -c)
    if [ "$core_gz_bytes" -gt "$CORE_MAX_GZIP_BYTES" ]; then
        echo "ERROR: core wasm gzip ${core_gz_bytes} B exceeds budget ${CORE_MAX_GZIP_BYTES} B." >&2
        echo "       Typst or another heavy dep has leaked into the core (no-features) build." >&2
        exit 1
    fi
    echo "Core size budget OK: ${core_gz_bytes} B <= ${CORE_MAX_GZIP_BYTES} B gzip"
fi
