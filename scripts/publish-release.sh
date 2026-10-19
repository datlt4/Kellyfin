#!/usr/bin/env bash
# Publishes the package.json version on GitHub from local builds (GitHub Actions is off):
# branch release/vX.Y.Z + tag at the current commit, a release, and every file in dist-mobile/ and dist-desktop/.
# Re-running uploads missing/changed files to the same release. Token: ~/.config/kellyfin/github-token
set -euo pipefail
cd "$(dirname "$0")/.."
REPO=datlt4/Kellyfin
TAG="v$(node -p "require('./package.json').version")"
SHA=$(git rev-parse HEAD)
TOKEN=$(cat "$HOME/.config/kellyfin/github-token")
gh() { curl -fsS -H "Authorization: Bearer $TOKEN" -H "Accept: application/vnd.github+json" "$@"; }
API=https://api.github.com/repos/$REPO

git diff --quiet HEAD || { echo "commit your changes first"; exit 1; }
git push -q origin HEAD
for ref in "heads/release/$TAG" "tags/$TAG"; do
  gh "$API/git/ref/$ref" >/dev/null 2>&1 ||
    { gh -X POST "$API/git/refs" -d "{\"ref\":\"refs/$ref\",\"sha\":\"$SHA\"}" >/dev/null && echo "created $ref"; }
done

ID=$(gh "$API/releases/tags/$TAG" 2>/dev/null | node -e 'try{console.log(JSON.parse(require("fs").readFileSync(0)).id)}catch{}' || true)
if [ -z "$ID" ]; then
  ID=$(gh -X POST "$API/releases" -d "{\"tag_name\":\"$TAG\",\"name\":\"Kellyfin $TAG\",\"generate_release_notes\":true,\"make_latest\":\"true\"}" |
    node -e 'console.log(JSON.parse(require("fs").readFileSync(0)).id)')
  echo "release $TAG created"
fi

existing=$(gh "$API/releases/$ID/assets?per_page=100" | node -e 'for (const a of JSON.parse(require("fs").readFileSync(0))) console.log(a.id, a.name)')
shopt -s nullglob
for f in dist-mobile/* dist-desktop/*; do
  name=$(basename "$f")
  old=$(echo "$existing" | awk -v n="$name" '$2 == n { print $1 }')
  [ -n "$old" ] && gh -X DELETE "$API/releases/assets/$old" >/dev/null
  gh -X POST -H "Content-Type: application/octet-stream" --data-binary @"$f" \
    "https://uploads.github.com/repos/$REPO/releases/$ID/assets?name=$name" >/dev/null && echo "uploaded $name"
done
echo "https://github.com/$REPO/releases/tag/$TAG"
