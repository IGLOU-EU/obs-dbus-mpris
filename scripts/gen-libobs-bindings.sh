#!/usr/bin/env bash
# Regenerates crates/libobs-sys/src/bindings.rs from the OBS headers (D-01),
# to run when the major version of OBS changes.
#
# Usage: scripts/gen-libobs-bindings.sh [obs-include-dir]
#
# Without argument, the headers come from the OBS Flatpak (com.obsproject.Studio).
# Otherwise, give the directory that holds obs-module.h, such as
# /usr/include/obs for a native OBS. Needs libclang.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
generator="$root/tools/gen-libobs-bindings"
output="$root/crates/libobs-sys/src/bindings.rs"

fail() {
    echo "error: $*" >&2
    exit 1
}

include_dir=${1:-}
if [[ -z $include_dir ]]; then
    location=$(flatpak info --show-location com.obsproject.Studio 2>/dev/null) ||
        fail "OBS Flatpak (com.obsproject.Studio) not found. Pass the OBS include directory, the one that holds obs-module.h."
    include_dir="$location/files/include/obs"
fi
[[ -f $include_dir/obs-module.h ]] || fail "no obs-module.h in $include_dir"

api_version() {
    sed -n "s/^#define LIBOBS_API_$1_VER \([0-9]*\).*/\1/p" "$include_dir/obs-config.h"
}
echo "Generating the libobs bindings from the OBS $(api_version MAJOR).$(api_version MINOR).$(api_version PATCH) headers in $include_dir"

# The generator's own target directory, under the ignored one of the workspace.
cargo run --quiet --locked \
    --manifest-path "$generator/Cargo.toml" \
    --target-dir "$root/target/gen-libobs-bindings" \
    -- "$include_dir" "$output"
# Same formatting as cargo fmt, so a later cargo fmt leaves the file alone.
rustfmt --edition 2024 "$output"

echo "Wrote $output"
