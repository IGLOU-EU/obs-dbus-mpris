# shellcheck shell=bash disable=SC2034 # its variables are read by the scripts that source it
# Shared by the scripts that build, check and lay out the plugin library
# (install-dev.sh, package-native.sh, build-flatpak.sh), so that every copy
# OBS may load passes the same checks. Sourced, never run.
#
# The library runs against the libobs and the glibc of its target, not of
# the build host, so it is refused when it needs a newer glibc than the
# target has, a libobs symbol the target's libobs lacks, or when it lacks an
# entry point OBS requires.

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
module=obs-dbus-mpris
built="${CARGO_TARGET_DIR:-$root/target}/release/libobs_mpris.so"
locales="$root/crates/obs-mpris/data/locale"
# The exports OBS refuses to load a module without.
required_exports=(obs_module_load obs_module_ver obs_module_set_pointer)
# glibc of org.freedesktop.Platform 25.08, the runtime of OBS Flatpak 32.
flatpak_glibc=2.42

fail() {
    echo "error: $*" >&2
    exit 1
}

# The version set once for the whole workspace.
workspace_version() {
    sed -n '/^\[workspace\.package\]/,/^\[/ s/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml"
}

build_plugin() {
    echo "Building $module in release"
    cargo build --release --locked --manifest-path "$root/Cargo.toml" -p obs-mpris
    [[ -f $built ]] || fail "no library at $built after the build"
}

# The libobs of OBS Flatpak, or nothing when it is not installed.
flatpak_libobs() {
    local location
    location=$(flatpak info --show-location com.obsproject.Studio 2>/dev/null) || return 0
    echo "$location/files/lib/libobs.so.30"
}

# The libobs of an OBS installed on the host, or nothing.
native_libobs() {
    ldconfig -p 2>/dev/null | sed -n 's/^[[:space:]]*libobs\.so\.[0-9]* .*=> //p' | head -n 1
}

# Whether version $1 is strictly newer than version $2.
newer_than() {
    [[ $1 != "$2" && $(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n 1) == "$1" ]]
}

# check_plugin LIBRARY GLIBC_MAX LIBOBS DESCRIPTION
# Fails when OBS would refuse LIBRARY on a target whose glibc is GLIBC_MAX.
# An empty LIBOBS skips the libobs check alone, for a build host without OBS.
check_plugin() {
    local library=$1 glibc_max=$2 libobs=$3 description=$4
    local refused=0 glibc_needed expected provided missing exports symbol

    [[ -f $library ]] || fail "no library at $library"
    echo "Checking $library against $description"

    # Every glibc version the library names must exist at load time, even
    # for weak symbols: the loader checks the whole list of version
    # references.
    glibc_needed=$(objdump -p "$library" | sed -n '/^Version References:/,/^$/p' |
        grep -o 'GLIBC_[0-9.]*' | sed 's/^GLIBC_//' | sort -uV | tail -n 1)
    if newer_than "$glibc_needed" "$glibc_max"; then
        echo "  FAIL glibc: needs GLIBC_$glibc_needed, the target has $glibc_max. Symbols too new:"
        nm -D --undefined-only "$library" | grep -o '[^ ]*@GLIBC_[0-9.]*' | while IFS=@ read -r symbol version; do
            if newer_than "${version#GLIBC_}" "$glibc_max"; then echo "         $symbol@$version"; fi
        done
        refused=1
    else
        echo "  ok   glibc: needs at most GLIBC_$glibc_needed, the target has $glibc_max"
    fi

    # Undefined symbols without a version are the libobs ones, resolved in
    # the OBS process. Weak ones may stay unresolved and are left out.
    if [[ -z $libobs ]]; then
        echo "  skip libobs: no libobs on this host to check the symbols against"
    elif [[ ! -f $libobs ]]; then
        echo "  FAIL libobs: not found at $libobs"
        refused=1
    else
        expected=$(nm -D --undefined-only "$library" | awk '$1 == "U" && $2 !~ /@/ { print $2 }' | sort -u)
        provided=$(nm -D --defined-only "$libobs" | awk '{ sub(/@.*/, "", $3); print $3 }' | sort -u)
        missing=$(comm -23 <(echo "$expected") <(echo "$provided"))
        if [[ -n $missing ]]; then
            echo "  FAIL libobs: symbols missing from $libobs:"
            while read -r symbol; do echo "         $symbol"; done <<<"$missing"
            refused=1
        else
            echo "  ok   libobs: all $(echo "$expected" | wc -l) symbols found in $libobs"
        fi
    fi

    exports=$(nm -D --defined-only "$library" | awk '{ print $3 }')
    for symbol in "${required_exports[@]}"; do
        if grep -qx "$symbol" <<<"$exports"; then
            echo "  ok   export: $symbol"
        else
            echo "  FAIL export: $symbol is not exported"
            refused=1
        fi
    done

    ((refused == 0)) || fail "OBS would fail to load $library"
}

# install_plugin DESTINATION
# Lays the built library and its translations out as OBS reads a module from
# a user plugin folder.
install_plugin() {
    local destination=$1
    mkdir -p "$destination/bin/64bit" "$destination/data/locale"
    # install replaces the file instead of rewriting it in place, which
    # spares an OBS that still has the previous version loaded.
    install -m 755 "$built" "$destination/bin/64bit/$module.so"
    install -m 644 "$locales"/*.ini "$destination/data/locale/"
}
