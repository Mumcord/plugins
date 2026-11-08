#!/usr/bin/env bash
# Tells whether a plugin can be tested with the pinned Mumcord (e2e/mumcord-version). Prints one
# line with the reason when the pinned tag is older than what the plugin needs, nothing otherwise.
#
# A plugin needs a newer Mumcord when
#   - its manifest `api` is a plugin API the pinned tag does not have (the table below), or
#   - with --e2e: `<plugin>/e2e/min-mumcord` names a newer tag (its E2E specs need host features
#     that only that release has).
#
# Usage: e2e/mumcord-gate.sh [--e2e] <plugin>
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
e2e=false
if [ "${1:-}" = --e2e ]; then
  e2e=true
  shift
fi
id="${1:?usage: e2e/mumcord-gate.sh [--e2e] <plugin>}"
tag="$(tr -d '[:space:]' < "$repo/e2e/mumcord-version")"

# The first Mumcord release with each plugin API newer than 1.0. Add a line when a new API ships.
api="$(jq -r '.api // ""' "$repo/$id/manifest.json")"
case "$api" in
  *1.2*) need=v0.8.0 ;;
  *1.1*) need=v0.7.0 ;;
  *) need="" ;;
esac
why="plugin API $api"
if "$e2e" && [ -f "$repo/$id/e2e/min-mumcord" ]; then
  min="$(tr -d '[:space:]' < "$repo/$id/e2e/min-mumcord")"
  if [ -z "$need" ] || [ "$(printf '%s\n' "$need" "$min" | sort -V | tail -n 1)" = "$min" ]; then
    need="$min"
    why="$id/e2e/min-mumcord"
  fi
fi

# Older: different from the need and sorting first.
if [ -n "$need" ] && [ "$tag" != "$need" ] &&
  [ "$(printf '%s\n' "$tag" "$need" | sort -V | head -n 1)" = "$tag" ]; then
  echo "skipping $id: needs Mumcord $need ($why), the pinned Mumcord is $tag (e2e/mumcord-version)"
fi
