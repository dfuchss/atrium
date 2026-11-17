#!/usr/bin/env bash
# Installs the Windows x86_64 build of Trunk into Cargo's bin folder (on the PATH of the runner)
# after checking its SHA-256. Runs in Git Bash.
set -euo pipefail

version=v0.22.1
sha256=1cc65012a9df60b02db948f5a3f0601e594ce952efb1cfd23147c9021c9f7d75

archive_name="trunk-x86_64-pc-windows-msvc.zip"
download_directory="$(mktemp -d)"
trap 'rm -rf "${download_directory}"' EXIT
archive="${download_directory}/${archive_name}"
bin_directory="${CARGO_HOME:-${HOME}/.cargo}/bin"

curl -fsSL -o "${archive}" "https://github.com/trunk-rs/trunk/releases/download/${version}/${archive_name}"
echo "${sha256}  ${archive}" | sha256sum --check --strict
mkdir -p "${bin_directory}"
unzip -o -q "${archive}" trunk.exe -d "${bin_directory}"
trunk --version
