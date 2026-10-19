#!/usr/bin/env bash
# Sets the app version everywhere it is declared. Usage: scripts/set-version.sh 0.3.0
set -euo pipefail
v=${1:?usage: $0 MAJOR.MINOR.PATCH}
[[ $v =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "not a semver: $v" >&2; exit 1; }
cd "$(dirname "$0")/.."
npm version "$v" --no-git-tag-version --allow-same-version >/dev/null
sed -i "0,/^version = \".*\"/s//version = \"$v\"/" server/Cargo.toml src-tauri/Cargo.toml
sed -i "s/^  \"version\": \".*\",/  \"version\": \"$v\",/" src-tauri/tauri.conf.json
perl -0pi -e "s/(name = \"kellyfin-server\"\nversion = )\"[^\"]+\"/\\1\"$v\"/" server/Cargo.lock
perl -0pi -e "s/(name = \"kellyfin\"\nversion = )\"[^\"]+\"/\\1\"$v\"/" src-tauri/Cargo.lock
grep -H '"version"\|^version' package.json server/Cargo.toml src-tauri/Cargo.toml src-tauri/tauri.conf.json
