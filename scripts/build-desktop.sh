#!/usr/bin/env bash
# Builds desktop installers locally into dist-desktop/.
#   scripts/build-desktop.sh linux      deb, rpm, AppImage for this machine's CPU (Docker, Ubuntu 22.04)
#   scripts/build-desktop.sh linux-arm64  same for arm64 (Docker under QEMU emulation: slow)
#   scripts/build-desktop.sh windows    NSIS installers + portable exe for x64 and arm64 (Docker, cargo-xwin)
#   scripts/build-desktop.sh macos      universal (Intel + Apple Silicon) .app/.dmg, on a Mac with Xcode
set -euo pipefail
cd "$(dirname "$0")/.."
V=$(node -p "require('./package.json').version")
OUT=dist-desktop
mkdir -p "$OUT"

in_docker() { # platform, command, [base image]
  local base=${3:-ubuntu:22.04}
  local img="kellyfin-desktop-builder:${1#linux/}-${base#ubuntu:}"
  docker build -q --platform "$1" --build-arg BASE="$base" -t "$img" -f scripts/desktop.Dockerfile scripts >/dev/null
  # node_modules lives in a volume so the host's (other OS/arch binaries) is left alone;
  # outputs are handed back to the calling user.
  docker run --rm --platform "$1" -v "$PWD:/src" -w /src \
    -v "kellyfin-node-${1#linux/}:/src/node_modules" -v "kellyfin-cargo-${1#linux/}:/usr/local/cargo/registry" \
    "$img" bash -c "npm ci --no-audit --no-fund && $2; rc=\$?; chown -R $(id -u):$(id -g) src-tauri/target src-tauri/gen dist 2>/dev/null; exit \$rc"
}

collect() { # dir glob...
  local d=$1; shift
  for g in "$@"; do find "$d" -maxdepth 3 -type f -name "$g" -exec cp {} "$OUT/" \; ; done
}

case "${1:-}" in
  linux | linux-arm64)
    P=linux/amd64; [ "$1" = linux-arm64 ] && P=linux/arm64
    in_docker "$P" "npx tauri build --bundles deb,rpm,appimage"
    collect src-tauri/target/release/bundle "*.deb" "*.rpm" "*.AppImage"
    ;;
  windows)
    for T in x86_64-pc-windows-msvc aarch64-pc-windows-msvc; do
      in_docker linux/amd64 "npx tauri build --runner cargo-xwin --target $T --bundles nsis" ubuntu:24.04
      collect "src-tauri/target/$T/release/bundle" "*.exe"
      A=$([ "$T" = x86_64-pc-windows-msvc ] && echo x64 || echo arm64)
      cp "src-tauri/target/$T/release/kellyfin.exe" "$OUT/kellyfin_${V}_windows_${A}_portable.exe"
    done
    ;;
  macos)
    npm ci --no-audit --no-fund
    rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
    CI=true npx tauri build --target universal-apple-darwin
    collect src-tauri/target/universal-apple-darwin/release/bundle "*.dmg"
    (cd src-tauri/target/universal-apple-darwin/release/bundle/macos && tar -czf "$OLDPWD/$OUT/kellyfin_${V}_macos_universal.app.tar.gz" Kellyfin.app)
    ;;
  *) echo "usage: $0 linux|linux-arm64|windows|macos"; exit 2 ;;
esac
ls -la "$OUT"
