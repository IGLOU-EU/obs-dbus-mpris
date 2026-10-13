#!/usr/bin/env bash
# Builds the plugin in release and installs it in the user plugin folder of
# OBS, once sure that OBS will be able to load it.
#
# Usage: scripts/install-dev.sh [--native]
#
# By default the target is OBS Flatpak (com.obsproject.Studio), which loads
# the native library from its user plugin folder (architecture.md, Builds).
# With --native, the target is an OBS installed on the host.
#
# The library runs against the libobs and the glibc of the target, not of
# the build host, so it is refused when it needs a newer glibc than the
# target has, a libobs symbol the target's libobs lacks, or when it lacks an
# entry point OBS requires.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
module=obs-dbus-mpris
built="$root/target/release/libobs_mpris.so"
locales="$root/crates/obs-mpris/data/locale"
# The exports OBS refuses to load a module without.
required_exports=(obs_module_load obs_module_ver obs_module_set_pointer)

fail() {
    echo "error: $*" >&2
    exit 1
}

case ${1:-} in
"") target=flatpak ;;
--native) target=native ;;
*) fail "unknown argument '$1'. Usage: scripts/install-dev.sh [--native]" ;;
esac

if [[ $target == flatpak ]]; then
    location=$(flatpak info --show-location com.obsproject.Studio 2>/dev/null) ||
        fail "OBS Flatpak (com.obsproject.Studio) is not installed. Use --native for an OBS installed on the host."
    libobs="$location/files/lib/libobs.so.30"
    # glibc of org.freedesktop.Platform 25.08, the runtime of OBS Flatpak 32.
    glibc_max=2.42
    plugins="$HOME/.var/app/com.obsproject.Studio/config/obs-studio/plugins"
    description="OBS Flatpak (runtime glibc $glibc_max)"
else
    libobs=$(ldconfig -p | sed -n 's/^[[:space:]]*libobs\.so\.[0-9]* .*=> //p' | head -n 1)
    [[ -n $libobs ]] || fail "no libobs found on the host. Is OBS installed natively?"
    glibc_max=$(getconf GNU_LIBC_VERSION | cut -d ' ' -f 2)
    plugins="${XDG_CONFIG_HOME:-$HOME/.config}/obs-studio/plugins"
    description="native OBS (host glibc $glibc_max)"
fi
[[ -f $libobs ]] || fail "libobs not found at $libobs"

echo "Building $module in release"
cargo build --release --locked --manifest-path "$root/Cargo.toml" -p obs-mpris
[[ -f $built ]] || fail "no library at $built after the build"

# Whether version $1 is strictly newer than version $2.
newer_than() {
    [[ $1 != "$2" && $(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n 1) == "$1" ]]
}

echo "Checking the library against $description"
refused=0

# Every glibc version the library names must exist at load time, even for
# weak symbols: the loader checks the whole list of version references.
glibc_needed=$(objdump -p "$built" | sed -n '/^Version References:/,/^$/p' |
    grep -o 'GLIBC_[0-9.]*' | sed 's/^GLIBC_//' | sort -uV | tail -n 1)
if newer_than "$glibc_needed" "$glibc_max"; then
    echo "  FAIL glibc: needs GLIBC_$glibc_needed, the target has $glibc_max. Symbols too new:"
    nm -D --undefined-only "$built" | grep -o '[^ ]*@GLIBC_[0-9.]*' | while IFS=@ read -r symbol version; do
        if newer_than "${version#GLIBC_}" "$glibc_max"; then echo "         $symbol@$version"; fi
    done
    refused=1
else
    echo "  ok   glibc: needs at most GLIBC_$glibc_needed, the target has $glibc_max"
fi

# Undefined symbols without a version are the libobs ones, resolved in the
# OBS process. Weak ones may stay unresolved and are left out.
expected=$(nm -D --undefined-only "$built" | awk '$1 == "U" && $2 !~ /@/ { print $2 }' | sort -u)
provided=$(nm -D --defined-only "$libobs" | awk '{ sub(/@.*/, "", $3); print $3 }' | sort -u)
missing=$(comm -23 <(echo "$expected") <(echo "$provided"))
if [[ -n $missing ]]; then
    echo "  FAIL libobs: symbols missing from $libobs:"
    while read -r symbol; do echo "         $symbol"; done <<<"$missing"
    refused=1
else
    echo "  ok   libobs: all $(echo "$expected" | wc -l) symbols found in $libobs"
fi

exports=$(nm -D --defined-only "$built" | awk '{ print $3 }')
for symbol in "${required_exports[@]}"; do
    if grep -qx "$symbol" <<<"$exports"; then
        echo "  ok   export: $symbol"
    else
        echo "  FAIL export: $symbol is not exported"
        refused=1
    fi
done

((refused == 0)) || fail "not installed: OBS would fail to load this library"

destination="$plugins/$module"
mkdir -p "$destination/bin/64bit" "$destination/data/locale"
# install replaces the file instead of rewriting it in place, which spares
# an OBS that still has the previous version loaded.
install -m 755 "$built" "$destination/bin/64bit/$module.so"
install -m 644 "$locales"/*.ini "$destination/data/locale/"

echo "Installed in $destination"
echo "OBS loads plugins at startup: restart it if it is running."
