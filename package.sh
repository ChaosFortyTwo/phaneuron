#!/usr/bin/env bash
# Build for the machine you are on and assemble a folder you can copy to a drive.
#   ./package.sh            -> dist/<this-os>/
set -euo pipefail
cd "$(dirname "$0")"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)  name=linux-x86_64 ;;
  Darwin-arm64)  name=macos-apple-silicon ;;
  MINGW*|MSYS*|CYGWIN*) name=windows-x86_64 ;;
  *) echo "unsupported platform: $(uname -s)-$(uname -m)"; exit 1 ;;
esac
exe=tinyvoxel; [[ $name == windows-* ]] && exe=tinyvoxel.exe

cargo build --release
out="dist/$name"
rm -rf "$out"; mkdir -p "$out/mods"
cp "target/release/$exe" "$out/"
cp -r assets examples docs "$out/"
cp README.md "$out/"
echo "ready: $out  ($(du -sh "$out" | cut -f1))"
