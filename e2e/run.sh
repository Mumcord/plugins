#!/usr/bin/env bash
# Runs the official plugins' E2E specs (<plugin>/e2e/*.e2e.ts) in a real Mumcord against a local
# Mumble server, using Mumcord's own E2E harness (Docker image, WebDriver, test servers, helpers).
#
# Mumcord is the release tag in e2e/mumcord-version, built from source with its test backend: a
# released binary has no WebDriver backend, and a tag never changes under the tests.
#
# Usage: e2e/run.sh [--check] [plugin ...]   (default: every plugin with an e2e/ folder)
#   --check   only type-checks and lints the specs inside the Mumcord checkout (no app, seconds)
# Environment:
#   MUMCORD_PLUGINS_E2E   work folder (default ~/.cache/mumcord-build/plugins-e2e): the Mumcord
#                         checkout, the built packages and the CLI's target dir
#   MUMCORD_PLUGIN_CLI    a `mumcord-plugin` binary of the pinned tag (default: built from the checkout)
#   MUMCORD_E2E_STEP      passed on to Mumcord's e2e/run.sh: `build` builds the image and the app
#                         only, `test` runs the specs with an image and app built earlier (CI)
#   MUMCORD_E2E_CACHE     Mumcord's E2E build cache (see its e2e/README.md)
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tag="$(tr -d '[:space:]' < "$repo/e2e/mumcord-version")"
work="${MUMCORD_PLUGINS_E2E:-$HOME/.cache/mumcord-build/plugins-e2e}"
mumcord="$work/mumcord-$tag"
packages="$work/packages"

check=false
if [ "${1:-}" = --check ]; then
  check=true
  shift
fi
plugins=("$@")
if [ ${#plugins[@]} -eq 0 ]; then
  for dir in "$repo"/*/e2e; do plugins+=("$(basename "$(dirname "$dir")")"); done
fi
for id in "${plugins[@]}"; do
  [ -d "$repo/$id/e2e" ] || { echo "no E2E specs in $id/e2e" >&2; exit 2; }
done

if [ ! -d "$mumcord/.git" ]; then
  echo "fetching Mumcord $tag" >&2
  mkdir -p "$work"
  git -c advice.detachedHead=false clone --quiet --depth 1 --branch "$tag" https://github.com/Mumcord/mumcord.git "$mumcord"
fi
[ -d "$mumcord/node_modules" ] || (cd "$mumcord" && npm ci --silent --no-audit --no-fund)

# The specs run as part of Mumcord's suite: e2e/specs/official-<plugin>/ sits next to Mumcord's own
# spec folders, so `../../helpers/...` resolves to its helpers.
rm -rf "$mumcord"/e2e/specs/official-*
mkdir -p "$mumcord/e2e/specs/official-shared"
cp "$repo"/e2e/helpers/*.ts "$mumcord/e2e/specs/official-shared/"
specs=()
for id in "${plugins[@]}"; do
  mkdir -p "$mumcord/e2e/specs/official-$id"
  cp "$repo/$id"/e2e/*.e2e.ts "$mumcord/e2e/specs/official-$id/"
  for f in "$mumcord/e2e/specs/official-$id"/*.e2e.ts; do specs+=(--spec "e2e/specs/${f#"$mumcord/e2e/specs/"}"); done
done

if "$check"; then
  cd "$mumcord"
  npx tsc -p e2e/tsconfig.json --noEmit
  npx eslint e2e/specs/official-*
  npx prettier --check e2e/specs/official-*
  exit 0
fi

if [ "${MUMCORD_E2E_STEP:-}" != build ]; then
  # Mumcord's e2e-plugin-registry.sh takes the CLI and the packages from this folder
  # (MUMCORD_E2E_PLUGIN_PACKAGES) instead of building them: only the plugins under test, built
  # from this checkout. Packages named eve-fit-* stay staged; its spec publishes them.
  rm -rf "$packages"
  mkdir -p "$packages/bin" "$packages/staged"
  cli="${MUMCORD_PLUGIN_CLI:-}"
  if [ -z "$cli" ]; then
    echo "building the mumcord-plugin CLI of $tag" >&2
    CARGO_TARGET_DIR="$work/target" cargo build --quiet --manifest-path "$mumcord/Cargo.toml" -p mumcord-plugin
    cli="$work/target/debug/mumcord-plugin"
  fi
  cp "$cli" "$packages/bin/mumcord-plugin"
  for id in "${plugins[@]}"; do
    echo "building $id" >&2
    "$cli" build "$repo/$id" --out "$packages/staged" >&2
  done
  export MUMCORD_E2E_PLUGIN_PACKAGES="$packages"
fi

cd "$mumcord"
if [ "${MUMCORD_E2E_STEP:-}" = build ]; then
  exec npm run e2e
fi
exec npm run e2e -- "${specs[@]}"
