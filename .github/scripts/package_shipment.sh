#!/usr/bin/env bash
# Package this Shipment as the release zip mercs.ink syncs and Modkit installs, then check the zip the way
# they will: unpacked into an empty directory, read with `qm manifest-info`, linted with `qm lint`.
#
#   .github/scripts/package_shipment.sh <qm> <out-dir>
#
# Run from the Shipment root, after client/scripts/build_asi.sh has placed the ASI in src/. The zip is
# <out-dir>/<shipment.name>-v<shipment.version>.zip — the name mercs.ink and Modkit select without opening
# any other asset — with manifest.yaml at its root. In GitHub Actions it also writes `asset=<path>` to
# $GITHUB_OUTPUT.
set -euo pipefail
[ "$#" -eq 2 ] || { echo "usage: $0 <qm> <out-dir>" >&2; exit 2; }
qm="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
out="$(cd "$2" && pwd)"
[ -f manifest.yaml ] || { echo "error: no manifest.yaml in $(pwd); run from the Shipment root" >&2; exit 1; }

# The DLC's converted bytes are EA's and never ship. src/level is where the converter writes them.
if [ -e src/level ]; then
  echo "error: src/level exists — the DLC's converted files must never enter a release" >&2
  exit 1
fi

info=$("$qm" manifest-info manifest.yaml)
name=$(jq -er .name <<<"$info")
version=$(jq -er .version <<<"$info")
asset="$out/$name-v$version.zip"
[ ! -e "$asset" ] || { echo "error: $asset already exists" >&2; exit 1; }

files=(manifest.yaml src README.md NOTICE)
for f in "${files[@]}"; do
  [ -e "$f" ] || { echo "error: $f is missing; the release zip carries it" >&2; exit 1; }
done
zip -r -X "$asset" "${files[@]}" -x '*.DS_Store' '*Thumbs.db'

entries=$(unzip -Z1 "$asset")
if grep -q '^src/level/' <<<"$entries"; then
  echo "error: $asset holds entries under src/level/" >&2
  exit 1
fi

# Check the zip as a player's Modkit receives it.
check="$(mktemp -d)"
trap 'rm -rf "$check"' EXIT
unzip -q "$asset" -d "$check"
[ -f "$check/manifest.yaml" ] || { echo "error: $asset has no manifest.yaml at its root" >&2; exit 1; }
unpacked=$("$qm" manifest-info "$check/manifest.yaml")
if [ "$unpacked" != "$info" ]; then
  echo "error: the zip's manifest reads $unpacked, the tree's reads $info" >&2
  exit 1
fi
"$qm" lint "$check"

unzip -l "$asset"
sha256=$(shasum -a 256 "$asset")
echo "packaged: $sha256"
if [ -n "${GITHUB_OUTPUT:-}" ]; then
  echo "asset=$asset" >> "$GITHUB_OUTPUT"
fi
