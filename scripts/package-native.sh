#!/usr/bin/env bash
# Builds the plugin in release and packs it as the native release archive,
# obs-dbus-mpris-<version>-x86_64-linux.tar.gz, once sure that OBS will be
# able to load it (scripts/lib/plugin.sh).
#
# Usage: scripts/package-native.sh [output-dir]
#
# The archive unpacks into the user plugin folder of a native OBS,
# ~/.config/obs-studio/plugins/. output-dir defaults to target/dist.
#
# The library must load on distributions older than the build host, so it
# is refused when it needs a glibc newer than GLIBC_MAX, 2.36 by default:
# Debian 12, the image the release is built on. The libobs check runs
# against the OBS of this host, native or Flatpak, and is skipped without
# one.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR source=lib/plugin.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/plugin.sh"

(($# <= 1)) || fail "too many arguments. Usage: scripts/package-native.sh [output-dir]"
output=${1:-$root/target/dist}
glibc_max=${GLIBC_MAX:-2.36}
version=$(workspace_version)
[[ -n $version ]] || fail "no version in the [workspace.package] table of $root/Cargo.toml"
archive="$module-$version-x86_64-linux.tar.gz"

libobs=$(native_libobs)
[[ -n $libobs ]] || libobs=$(flatpak_libobs)

build_plugin
check_plugin "$built" "$glibc_max" "$libobs" "glibc $glibc_max (set GLIBC_MAX to aim at other systems)"

staging=$(mktemp -d)
trap 'rm -r "$staging"' EXIT
install_plugin "$staging/$module"
install -m 644 "$root/LICENSE" "$staging/$module/"

mkdir -p "$output"
# Owned by root in the archive, so that unpacking never carries over the
# user ids of the build machine.
tar --create --gzip --file "$output/$archive" --directory "$staging" \
    --sort=name --owner=0 --group=0 --numeric-owner "$module"

echo "Packed $output/$archive"
echo "Install: tar -xzf $archive -C ~/.config/obs-studio/plugins/"
