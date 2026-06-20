#!/bin/bash
set -e
set -o pipefail

# Builds the .NET binding end-to-end:
#
#   1. the native C-ABI cdylib  (cargo build -p quillmark-dotnet)
#   2. the managed Quillmark assembly + tests (dotnet build/test)
#
# The managed `Quillmark.csproj` copies the cargo-built native library next to
# its output via the `CopyNativeLibrary` MSBuild target, so the native build
# must run first. This mirrors `build-wasm.sh`: one Rust crate plus a
# hand-written language layer, packaged together.
#
# Usage:
#   ./scripts/build-dotnet.sh            # debug build + run tests
#   ./scripts/build-dotnet.sh --release  # release build + run tests

PROFILE_ARG=""
CARGO_PROFILE="debug"
for arg in "$@"; do
    case "$arg" in
        --release)
            PROFILE_ARG="--release"
            CARGO_PROFILE="release"
            ;;
        *)
            echo "Unknown argument: $arg" >&2
            exit 1
            ;;
    esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CSHARP_DIR="$ROOT/crates/bindings/dotnet/csharp"

echo "==> Building native cdylib (profile: $CARGO_PROFILE)"
cargo build -p quillmark-dotnet $PROFILE_ARG

if ! command -v dotnet >/dev/null 2>&1; then
    echo "==> dotnet SDK not found; skipping managed build/test." >&2
    echo "    Native library is built at target/$CARGO_PROFILE/." >&2
    exit 0
fi

echo "==> Building managed assembly"
dotnet build "$CSHARP_DIR/Quillmark/Quillmark.csproj" \
    -c "$([ "$CARGO_PROFILE" = release ] && echo Release || echo Debug)" \
    -p:CargoProfile="$CARGO_PROFILE"

echo "==> Running tests"
dotnet test "$CSHARP_DIR/Quillmark.Tests/Quillmark.Tests.csproj" \
    -c "$([ "$CARGO_PROFILE" = release ] && echo Release || echo Debug)" \
    -p:CargoProfile="$CARGO_PROFILE"
