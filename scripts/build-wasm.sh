#!/bin/bash
set -e
# pipefail so a failing `gzip` in `gzip -c … | wc -c` propagates instead of
# being masked by `wc`'s exit 0. Without it the core size-budget check below
# can silently read 0 bytes on a gzip failure and false-pass — defeating the
# one guard rail that catches a Typst leak into the no-features core build.
set -o pipefail

# Builds THREE wasm artifacts from the one crate (the as-built design is
# documented in docs/migrations/0.89-to-0.90.md):
#
#   pkg/core/             — no Typst: parse / load / validate / schema / seed / blueprint
#   pkg/backends/typst/   — Typst-backed engine + LiveSession + canvas (a private
#                           backend binary, NOT a public export)
#   pkg/backends/pdfform/ — Typst-free PDF-form backend (engine + LiveSession +
#                           canvas; the pdfform-preview feature adds the web-sys
#                           canvas painter over the always-linked hayro raster;
#                           private backend binary, NOT a public export)
#
# These generated artifacts plus the hand-written canonical layer ship as one
# npm package. Public surface: the root `.` export (`@quillmark/wasm`) is the
# canonical `Quill`/`Document`/`Engine` API (see pkg/runtime/), and `./core` is
# the render-free escape hatch. The backends are reached only internally, by the
# canonical layer's lazy `import("../backends/<id>/wasm.js")`.
#
# Profile selection. Default is the size-optimized release build used for
# npm publish. `--ci` switches to a fast-compiling profile for PR validation
# where only correctness matters. Keep these two paths in sync with the
# cache namespacing in .github/workflows/{ci,release}.yml.
PROFILE="wasm-release"
MODE_LABEL="release (size-optimized)"
RELEASE_STAMP=0
for arg in "$@"; do
    case "$arg" in
        --ci)
            PROFILE="wasm-ci"
            MODE_LABEL="ci (fast compile, unoptimized)"
            ;;
        --release-stamp)
            RELEASE_STAMP=1
            ;;
        *)
            echo "Unknown argument: $arg" >&2
            echo "Usage: $0 [--ci] [--release-stamp]" >&2
            exit 2
            ;;
    esac
done

echo "Building WASM modules for @quillmark/wasm... [profile: $MODE_LABEL]"

cd "$(dirname "$0")/.."

# Check for required tools. The CLI's version must match the wasm-bindgen
# crate in Cargo.lock; wasm-bindgen itself only detects a mismatch when it
# runs — after the multi-minute cargo build — so check it up front.
LOCKED_WBG=$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"/\1/p')
if ! command -v wasm-bindgen &> /dev/null; then
    echo "wasm-bindgen not found. Install it with:" >&2
    echo "  cargo install wasm-bindgen-cli --version $LOCKED_WBG" >&2
    exit 1
fi
CLI_WBG=$(wasm-bindgen --version | awk '{print $2}')
if [ "$CLI_WBG" != "$LOCKED_WBG" ]; then
    echo "ERROR: wasm-bindgen-cli $CLI_WBG does not match Cargo.lock's wasm-bindgen $LOCKED_WBG." >&2
    echo "  cargo install wasm-bindgen-cli --version $LOCKED_WBG" >&2
    exit 1
fi
if ! command -v jq &> /dev/null; then
    echo "jq not found (needed to read the package version from cargo metadata)." >&2
    exit 1
fi

# Build one variant: cargo build with the given feature flags, then run
# wasm-bindgen into pkg/<subdir>/. Both variants emit the same wasm artifact
# name (quillmark_wasm.wasm) to the same target path, so they must run
# sequentially — each wasm-bindgen pass consumes the build before the next
# cargo build overwrites it.
#
# `--weak-refs` opts into FinalizationRegistry-based auto-free for
# wasm-bindgen handles; `.free()` is still emitted for deterministic teardown.
# (Runtime floor is the package.json `engines` field, not set here.)
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

# backends/typst   = default features (Typst).
# backends/pdfform = the Typst-free PDF-form backend with its canvas preview
#                    seam (pdfform-preview). Built like the typst variant: the
#                    same cargo build + wasm-bindgen pass, sequentially (every
#                    variant emits the same quillmark_wasm.wasm to the same
#                    target path, so they must not run concurrently).
# core             = no features (Typst excluded).
build_variant backends/typst
build_variant backends/pdfform --no-default-features --features pdfform-preview
build_variant core --no-default-features

# runtime = the canonical consumer API: a hand-written JS layer (NOT generated
# by wasm-bindgen) over core + the backend builds. It is plain source, so just
# copy it into pkg/ alongside the generated variants.
echo ""
echo "Copying variant: runtime (hand-written canonical API)"
mkdir -p pkg/runtime
cp crates/bindings/wasm/runtime/runtime.js pkg/runtime/runtime.js
cp crates/bindings/wasm/runtime/runtime.d.ts pkg/runtime/runtime.d.ts

# Note: a wasm-opt -Oz pass was tried and removed. With the current
# `wasm-release` profile (opt-level=z, fat LTO, codegen-units=1,
# panic=abort, strip=true) it saves only ~15 KB raw / ~10 KB gzipped
# (<0.1%) — not worth the build dependency or the extra build time.

# Extract version and create package.json from template. Cargo.toml carries
# the LAST RELEASED version, so a from-source build is ahead of the number it
# would stamp. Default: mark it — next patch plus `-dev.<short-sha>` — so a
# dev pkg/ can never pass for a published release (npm dedupe, peer ranges,
# humans debugging read an honest number). `--release-stamp` stamps the
# version verbatim; only release.yml passes it, from the bumped release tag,
# and asserts the stamp equals the tag before `npm publish`.
VERSION=$(cargo metadata --format-version=1 --no-deps | jq -r '.packages[] | select(.name == "quillmark-wasm") | .version')
if [ -z "$VERSION" ] || [ "$VERSION" = "null" ]; then
    echo "ERROR: could not determine quillmark-wasm version from cargo metadata." >&2
    exit 1
fi
if [ "$RELEASE_STAMP" -ne 1 ]; then
    BASE=${VERSION%%-*}
    IFS=. read -r MAJOR MINOR PATCH <<< "$BASE"
    SHA=$(git rev-parse --short HEAD 2>/dev/null || echo local)
    VERSION="$MAJOR.$MINOR.$((PATCH + 1))-dev.$SHA"
fi
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
echo "Output directory: pkg/  (core/ + backends/typst/ + backends/pdfform/ + runtime/)"
echo "Package version: $VERSION"

# Show sizes — transport size (gzip/brotli) is what matters for delivery.
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
report_size "core"            pkg/core/wasm_bg.wasm
report_size "typst backend"   pkg/backends/typst/wasm_bg.wasm
report_size "pdfform backend" pkg/backends/pdfform/wasm_bg.wasm

# Size budget on the core artifact: the split only pays off if core stays
# Typst-free. Typst is megabytes, so a leak back into the no-features build
# would blow past this ceiling — fail rather than ship it silently. The gzip
# ceiling sits well above core's real size and far below anything carrying Typst.
#
# Only enforced on the size-optimized release profile (where the artifact
# publishes); the `wasm-ci` profile is unoptimized, so its size is meaningless
# here.
CORE_MAX_GZIP_BYTES=${CORE_MAX_GZIP_BYTES:-1500000}
if [ -f pkg/core/wasm_bg.wasm ] && [ "$PROFILE" = "wasm-release" ]; then
    core_gz_bytes=$(gzip -9 -c pkg/core/wasm_bg.wasm | wc -c | tr -d '[:space:]')
    if ! [ "$core_gz_bytes" -gt 0 ] 2>/dev/null; then
        echo "ERROR: could not measure core wasm gzip size (got '${core_gz_bytes}')." >&2
        exit 1
    fi
    if [ "$core_gz_bytes" -gt "$CORE_MAX_GZIP_BYTES" ]; then
        echo "ERROR: core wasm gzip ${core_gz_bytes} B exceeds budget ${CORE_MAX_GZIP_BYTES} B." >&2
        echo "       Typst or another heavy dep has leaked into the core (no-features) build." >&2
        exit 1
    fi
    echo "Core size budget OK: ${core_gz_bytes} B <= ${CORE_MAX_GZIP_BYTES} B gzip"
fi
