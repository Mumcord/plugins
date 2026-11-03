#!/usr/bin/env bash
# Prints, as a JSON array, the official plugins whose tests must run for the changes since <base>:
# a plugin runs when a file under <plugin>/ changed; every plugin runs when a shared file changed
# (the E2E harness in e2e/, the pinned Mumcord tag, this workflow, the template).
# Without a usable base (a new branch, a manual run) every plugin runs.
#
# Usage: e2e/changed-plugins.sh [<base commit>]
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
base="${1:-}"

all=()
for manifest in "$repo"/*/manifest.json; do
  id="$(basename "$(dirname "$manifest")")"
  [ "$id" = template ] || all+=("$id")
done

json() {
  if [ $# -eq 0 ]; then echo "[]"; else printf '%s\n' "$@" | jq -R . | jq -cs .; fi
}

if [ -z "$base" ] || [ "$base" = 0000000000000000000000000000000000000000 ] ||
  ! git -C "$repo" cat-file -e "$base^{commit}" 2>/dev/null; then
  json "${all[@]}"
  exit 0
fi

changed="$(git -C "$repo" diff --name-only "$base" HEAD)"
if grep -qE '^(e2e/|template/|\.github/workflows/plugin-tests\.yml$)' <<<"$changed"; then
  json "${all[@]}"
  exit 0
fi
selected=()
for id in "${all[@]}"; do
  if grep -q "^$id/" <<<"$changed"; then selected+=("$id"); fi
done
json "${selected[@]}"
