#!/usr/bin/env bash
# Checks a built AppImage: its desktop entry, icon and binary, its GStreamer plugins and the
# AppImage type 2 magic. Fails unless it is given exactly one file, so a glob that matches no or
# several AppImages fails too. Extracting works without FUSE. Used by the CI workflows and
# scripts/linux-check.sh --appimage.
#
# The AppImage bundles every GStreamer plugin the build machine has, so a stray package could
# ship patent-encumbered codecs unnoticed: no plugin of the gst-plugins-ugly or gst-plugins-bad
# sets may be inside. A denylist, not an allowlist: the allowed set (base, good, libav) is large
# and differs between distributions and releases, while the encumbered plugins are few and keep
# their names. The list follows the current upstream split (GStreamer 1.26): plugins whose
# patents expired moved to the good set (mp3 with lame, mpg123, twolame and xingmux; amrnb and
# amrwbdec), so they are allowed.
# Usage: scripts/check-appimage.sh <AppImage>
set -euo pipefail

fail() { echo "$*" >&2; exit 1; }

[ "$#" -eq 1 ] && [ -f "$1" ] || fail "expected one AppImage, found: $*"
appimage="$(realpath "$1")"

extract_directory="$(mktemp -d)"
trap 'rm -rf "${extract_directory}"' EXIT
(cd "${extract_directory}" && APPIMAGE_EXTRACT_AND_RUN=1 "${appimage}" --appimage-extract > /dev/null)
contents="${extract_directory}/squashfs-root"

for expected_line in Name=Atrium Icon=atrium StartupWMClass=org.fuchss.atrium; do
  grep -qx "${expected_line}" "${contents}/atrium.desktop" || fail "atrium.desktop lacks ${expected_line}"
done
[ -f "${contents}/usr/share/icons/hicolor/512x512/apps/atrium.png" ] || fail "the 512 px icon is missing"
[ -x "${contents}/usr/bin/atrium" ] || fail "usr/bin/atrium is missing"

plugins="${contents}/usr/lib/gstreamer-1.0"
# Without them WebKitGTK's web processes crash on pages with media (appsink, autoaudiosink).
for plugin in app autodetect libav; do
  [ -f "${plugins}/libgst${plugin}.so" ] || fail "the GStreamer plugin libgst${plugin}.so is missing"
done
# The ugly set, and the bad set's codecs, muxers, demuxers and DVD/DVB sources.
forbidden_plugins=(
  a52dec asf cdio dvdlpcmdec dvdread dvdsub mpeg2dec realmedia sid x264
  openh264 x265 de265 svthevcenc fdkaac faac faad voaacenc dtsdec mpeg2enc
  asfmux mpegpsdemux mpegpsmux mpegtsdemux mpegtsmux dvb resindvd
)
for plugin in "${forbidden_plugins[@]}"; do
  [ ! -e "${plugins}/libgst${plugin}.so" ] || fail "the AppImage bundles the plugins-ugly/plugins-bad plugin libgst${plugin}.so"
done

# The AppImage type 2 magic at byte 8: AI followed by 0x02.
[ "$(xxd -s 8 -l 3 -p "${appimage}")" = "414902" ] || fail "${appimage} is not a type 2 AppImage"
echo "${appimage}: the AppImage checks passed"
