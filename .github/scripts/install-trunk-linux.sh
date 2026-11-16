#!/usr/bin/env bash
# Installs the static musl build of Trunk into /usr/local/bin after checking its SHA-256. The static
# build runs regardless of the host's glibc. Needs sudo.
set -euo pipefail

version=v0.22.1
architecture="$(uname -m)"
case "${architecture}" in
  x86_64) sha256=a1fe5cd6b053f5a76f66b598d296022a7f06f56def6013cc16cd6897d53f5363 ;;
  aarch64) sha256=8c84a939aaf48bbdac2d78195c932adb319e610fb2cbf791bae07b2660cf3e46 ;;
  *) echo "no Trunk build for ${architecture}" >&2; exit 1 ;;
esac

archive_name="trunk-${architecture}-unknown-linux-musl.tar.gz"
download_directory="$(mktemp -d)"
trap 'rm -rf "${download_directory}"' EXIT
archive="${download_directory}/${archive_name}"

curl -fsSL -o "${archive}" "https://github.com/trunk-rs/trunk/releases/download/${version}/${archive_name}"
echo "${sha256}  ${archive}" | sha256sum --check --strict
sudo tar -xz -f "${archive}" -C /usr/local/bin trunk
trunk --version
