#!/usr/bin/env bash
# Regenerates every raster icon from the SVG sources in app/src-tauri/icons/src.
#
# Needs macOS (iconutil), ImageMagick (`magick`, rendering SVG through Inkscape or librsvg).
# Run it after editing an SVG and commit the result.
#
#   atrium.svg         free-standing hexagon: Linux/general PNGs, About panel source, .ico (64+)
#   atrium-macos.svg   hexagon on the navy macOS squircle: icon.icns, About panel
#   atrium-small.svg   simplified hexagon for 16-32 px (also composed onto the squircle for icns,
#                      and the Linux and Windows tray icon)
#   atrium-tray.svg    monochrome menu-bar template icon (black on transparent)
#   badge-dot.svg      red unread dot, the Windows taskbar overlay
set -euo pipefail
cd "$(dirname "$0")/.."

SRC=app/src-tauri/icons/src
OUT=app/src-tauri/icons
ASSETS=crates/host-tauri/assets
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# render <svg> <size> <png>: transparent background, no resampling blur.
render() { magick -background none -density 144 "$1" -resize "${2}x${2}" -define png:color-type=6 "$3"; }

# The small variant on the squircle, for the 16 and 32 px icns entries: the same placement as the
# hexagon in atrium-macos.svg (scale 0.8, centred at 512,504), the small SVG's body inlined.
{
  echo '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">'
  echo '<defs><linearGradient id="grad-surface" gradientUnits="userSpaceOnUse" x1="0" y1="100" x2="0" y2="924">'
  echo '<stop offset="0" stop-color="#1E2A52"/><stop offset="1" stop-color="#0A1028"/></linearGradient></defs>'
  echo '<rect x="100" y="100" width="824" height="824" rx="185" fill="url(#grad-surface)"/>'
  echo '<g transform="translate(512 504) scale(0.8) translate(-512 -512)">'
  sed '1d;$d' "$SRC/atrium-small.svg"
  echo '</g></svg>'
} >"$TMP/macos-small.svg"

# Sources and Linux/general PNGs (free hexagon; the small variant below 64 px).
render "$SRC/atrium.svg" 1024 "$SRC/atrium-1024.png"
render "$SRC/atrium.svg" 512 "$OUT/icon.png"
render "$SRC/atrium.svg" 256 "$OUT/128x128@2x.png"
render "$SRC/atrium.svg" 128 "$OUT/128x128.png"
render "$SRC/atrium.svg" 64 "$OUT/64x64.png"
render "$SRC/atrium-small.svg" 32 "$OUT/32x32.png"

# macOS icns: squircle icon; the small variant for 16 and 32 px (1x and 2x of 16).
ICONSET="$TMP/icon.iconset"
mkdir "$ICONSET"
for size in 16 32 128 256 512; do
  for scale in 1 2; do
    px=$((size * scale))
    name="icon_${size}x${size}"
    [ "$scale" = 2 ] && name="${name}@2x"
    if [ "$px" -le 32 ]; then
      render "$TMP/macos-small.svg" "$px" "$ICONSET/$name.png"
    else
      render "$SRC/atrium-macos.svg" "$px" "$ICONSET/$name.png"
    fi
  done
done
iconutil -c icns "$ICONSET" -o "$OUT/icon.icns"

# Windows ico: small variant up to 48 px, full logo for 64 and 256.
for px in 16 24 32 48; do render "$SRC/atrium-small.svg" "$px" "$TMP/ico-$px.png"; done
for px in 64 256; do render "$SRC/atrium.svg" "$px" "$TMP/ico-$px.png"; done
magick "$TMP"/ico-16.png "$TMP"/ico-24.png "$TMP"/ico-32.png "$TMP"/ico-48.png \
  "$TMP"/ico-64.png "$TMP"/ico-256.png "$OUT/icon.ico"

# Native About panel (the dock icon look) and the menu-bar template icon (44 px = 22 pt at 2x).
render "$SRC/atrium-macos.svg" 512 "$ASSETS/about-icon.png"
render "$SRC/atrium-tray.svg" 44 "$ASSETS/tray-icon.png"
cp "$SRC/atrium-tray.svg" "$ASSETS/tray-icon.svg"
# Linux and Windows tray icon in colour (panels do not tint it), shown at about 22 px: the small
# variant.
render "$SRC/atrium-small.svg" 64 "$ASSETS/tray-icon-color.png"
# Windows taskbar overlay, 16 px at 200 % scaling.
render "$SRC/badge-dot.svg" 32 "$ASSETS/badge-dot.png"

echo "icons regenerated"
