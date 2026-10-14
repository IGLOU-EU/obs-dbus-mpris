#!/usr/bin/env bash
# Builds the Flatpak extension of the plugin offline, checks the library it
# holds as OBS Flatpak will load it (scripts/lib/plugin.sh), and packs it as
# the single-file bundle obs-dbus-mpris-<version>-x86_64.flatpak.
#
# Usage: scripts/build-flatpak.sh [work-dir]
#
# work-dir, target/flatpak by default, receives the build directory, the
# flatpak-builder state, the local repository and the bundle. Nothing is
# installed: the build needs OBS Flatpak (com.obsproject.Studio, branch
# stable), org.freedesktop.Sdk//25.08 and its rust-stable extension already
# installed.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR source=lib/plugin.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/plugin.sh"

(($# <= 1)) || fail "too many arguments. Usage: scripts/build-flatpak.sh [work-dir]"
work=${1:-$root/target/flatpak}
id=com.obsproject.Studio.Plugin.DbusMpris
manifest="$root/packaging/flatpak/$id.yml"
version=$(workspace_version)
[[ -n $version ]] || fail "no version in the [workspace.package] table of $root/Cargo.toml"
bundle="$work/$module-$version-x86_64.flatpak"

command -v flatpak-builder >/dev/null || fail "flatpak-builder is not installed"
libobs=$(flatpak_libobs)
[[ -n $libobs ]] || fail "OBS Flatpak (com.obsproject.Studio) is not installed: the extension builds against it"

mkdir -p "$work"
echo "Building $id offline"
# rofiles-fuse needs FUSE, which containers such as CI jobs rarely offer.
flatpak-builder --force-clean --disable-rofiles-fuse \
    --state-dir="$work/state" --repo="$work/repo" "$work/build" "$manifest"

check_plugin "$work/build/files/lib/obs-plugins/$module.so" "$flatpak_glibc" "$libobs" \
    "OBS Flatpak (runtime glibc $flatpak_glibc)"

flatpak build-bundle --runtime "$work/repo" "$bundle" "$id" stable
echo "Bundled $bundle"
echo "Install: flatpak install --user $bundle"
