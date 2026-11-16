#!/usr/bin/env bash
# Runs the Linux checks (formatting, clippy and tests) on a Linux host or in a distrobox, and with
# --appimage also builds the AppImage and checks its desktop entry, icon and binary.
#
# Needs the system packages, Rust (with the wasm32-unknown-unknown target), Trunk and the Tauri CLI
# from DEVELOPMENT.md ("Linux"). The AppImage check also needs the GStreamer plugins and tools
# named there and, for the start-up check, xvfb-run.
# Usage: scripts/linux-check.sh [--appimage]
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n==> %s\n' "$*"; }

build_appimage=false
case "${1:-}" in
  "") ;;
  --appimage) build_appimage=true ;;
  *) echo "usage: scripts/linux-check.sh [--appimage]" >&2; exit 2 ;;
esac

step "rustfmt"
cargo fmt --all --check

step "trunk build"
(cd app/ui && trunk build)

step "clippy (native)"
cargo clippy --workspace --all-targets -- -D warnings

step "clippy (wasm UI)"
cargo clippy -p atrium-ui --target wasm32-unknown-unknown -- -D warnings

step "tests"
cargo test --workspace --exclude atrium-ui

if [ "${build_appimage}" = true ]; then
  step "build the AppImage"
  # patchelf and file are run by the GStreamer bundling of linuxdeploy, jq and xxd by the checks below.
  for tool in patchelf file xxd jq; do
    command -v "${tool}" > /dev/null || { echo "${tool} not found (see Linux in DEVELOPMENT.md)" >&2; exit 1; }
  done
  # The GStreamer bundling of linuxdeploy only guesses the Debian layout; point it at Fedora's.
  if [ -z "${GSTREAMER_PLUGINS_DIR:-}" ] && [ -d /usr/lib64/gstreamer-1.0 ]; then
    export GSTREAMER_PLUGINS_DIR=/usr/lib64/gstreamer-1.0 GSTREAMER_HELPERS_DIR=/usr/libexec/gstreamer-1.0
  fi
  (cd app/src-tauri && cargo tauri build --bundles appimage)

  target_directory="${CARGO_TARGET_DIR:-target}"
  version="$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "atrium") | .version')"
  appimage_files=("${target_directory}"/release/bundle/appimage/atrium_"${version}"_*.AppImage)

  step "check the AppImage contents"
  scripts/check-appimage.sh "${appimage_files[@]}"

  if command -v xvfb-run > /dev/null; then
    step "start the AppImage for a few seconds"
    # A release build ignores ATRIUM_CONFIG_DIR, so the XDG folders keep it off the real config.
    # Without a session bus it skips the single-instance guard: a running Atrium would take over.
    xdg_directory="$(mktemp -d)"
    # timeout ends the app with status 124, which means it was still running.
    status=0
    XDG_CONFIG_HOME="${xdg_directory}/config" XDG_DATA_HOME="${xdg_directory}/data" \
      XDG_CACHE_HOME="${xdg_directory}/cache" APPIMAGE_EXTRACT_AND_RUN=1 \
      DBUS_SESSION_BUS_ADDRESS="unix:path=${xdg_directory}/no-bus" \
      xvfb-run -a timeout 8 "${appimage_files[0]}" > /dev/null 2>&1 || status=$?
    rm -rf "${xdg_directory}"
    [ "${status}" -eq 124 ] || { echo "the AppImage exited early with status ${status}" >&2; exit 1; }
  else
    echo "xvfb-run not found: skipped the start-up check"
  fi
fi

step "all Linux checks passed"
