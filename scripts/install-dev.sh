#!/usr/bin/env bash
# Builds the plugin in release and installs it in the user plugin folder of
# OBS, once sure that OBS will be able to load it (scripts/lib/plugin.sh).
#
# Usage: scripts/install-dev.sh [--native]
#
# By default the target is OBS Flatpak (com.obsproject.Studio), which loads
# the native library from its user plugin folder (architecture.md, Builds),
# unless the Flatpak extension of the plugin is installed: OBS then loads
# the extension and skips the copy, so the script refuses to install one.
# With --native, the target is an OBS installed on the host.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR source=lib/plugin.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/plugin.sh"

case ${1:-} in
"") target=flatpak ;;
--native) target=native ;;
*) fail "unknown argument '$1'. Usage: scripts/install-dev.sh [--native]" ;;
esac

if [[ $target == flatpak ]]; then
    libobs=$(flatpak_libobs)
    [[ -n $libobs ]] ||
        fail "OBS Flatpak (com.obsproject.Studio) is not installed. Use --native for an OBS installed on the host."
    # OBS loads the extension first, then skips a user folder copy of the
    # same module with nothing but a "Skipping legacy plugin" line in its
    # log: installing here would change nothing.
    ! flatpak info "$extension" >/dev/null 2>&1 ||
        fail "the extension $extension is installed, and OBS would load it instead of this copy. Remove it while developing: flatpak uninstall $extension"
    glibc_max=$flatpak_glibc
    plugins="$HOME/.var/app/com.obsproject.Studio/config/obs-studio/plugins"
    description="OBS Flatpak (runtime glibc $glibc_max)"
else
    libobs=$(native_libobs)
    [[ -n $libobs ]] || fail "no libobs found on the host. Is OBS installed natively?"
    glibc_max=$(getconf GNU_LIBC_VERSION | cut -d ' ' -f 2)
    plugins="${XDG_CONFIG_HOME:-$HOME/.config}/obs-studio/plugins"
    description="native OBS (host glibc $glibc_max)"
fi

build_plugin
check_plugin "$built" "$glibc_max" "$libobs" "$description"

destination="$plugins/$module"
install_plugin "$destination"

echo "Installed in $destination"
echo "OBS loads plugins at startup: restart it if it is running."
