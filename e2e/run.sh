#!/usr/bin/env bash
# Runs the official plugins' E2E specs (<plugin>/e2e/*.e2e.ts) in a real Mumcord against a local
# Mumble server, using Mumcord's own E2E harness (Docker image, WebDriver, test servers, helpers).
#
# Mumcord is the release tag in e2e/mumcord-version, built from source with its test backend: a
# released binary has no WebDriver backend, and a tag never changes under the tests. A plugin that
# needs a newer Mumcord (e2e/mumcord-gate.sh: its plugin API or <plugin>/e2e/min-mumcord) is
# skipped with a notice.
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
#   MUMCORD_REF           local runs only: a Mumcord branch or full commit hash to build instead
#                         of the tag, e.g. for an unreleased host feature. It gets its own work
#                         folder ($MUMCORD_PLUGINS_E2E/ref), E2E cache and image (unless set) and
#                         turns the version gate off.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tag="$(tr -d '[:space:]' < "$repo/e2e/mumcord-version")"
work="${MUMCORD_PLUGINS_E2E:-$HOME/.cache/mumcord-build/plugins-e2e}"
ref="${MUMCORD_REF:-}"
if [ -n "$ref" ]; then
  # Kept apart from the tag's checkout, build cache and image: a branch build must never replace
  # what the tag runs use.
  work="$work/ref"
  mumcord="$work/mumcord"
  export MUMCORD_E2E_CACHE="${MUMCORD_E2E_CACHE:-$work/e2e-cache}"
  export MUMCORD_E2E_IMAGE="${MUMCORD_E2E_IMAGE:-mumcord-e2e-ref}"
  tag="$ref"
else
  mumcord="$work/mumcord-$tag"
fi
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
runnable=()
for id in "${plugins[@]}"; do
  [ -d "$repo/$id/e2e" ] || { echo "no E2E specs in $id/e2e" >&2; exit 2; }
  skip=""
  [ -n "$ref" ] || skip="$("$repo/e2e/mumcord-gate.sh" --e2e "$id")"
  if [ -n "$skip" ]; then echo "notice: $skip" >&2; else runnable+=("$id"); fi
done
if [ ${#runnable[@]} -eq 0 ]; then
  echo "no plugin to test with Mumcord $tag" >&2
  exit 0
fi
plugins=("${runnable[@]}")

if [ -n "$ref" ]; then
  # A branch moves: fetch it on every run.
  [ -d "$mumcord/.git" ] || git init --quiet "$mumcord"
  echo "fetching Mumcord $ref" >&2
  git -C "$mumcord" fetch --quiet --depth 1 https://github.com/Mumcord/mumcord.git "$ref"
  git -C "$mumcord" -c advice.detachedHead=false checkout --quiet --force FETCH_HEAD
elif [ ! -d "$mumcord/.git" ]; then
  echo "fetching Mumcord $tag" >&2
  mkdir -p "$work"
  git -c advice.detachedHead=false clone --quiet --depth 1 --branch "$tag" https://github.com/Mumcord/mumcord.git "$mumcord"
fi
# Reinstalled when the lock file changed (a moved branch).
lock="$(git -C "$mumcord" rev-parse HEAD:package-lock.json)"
if [ "$(cat "$mumcord/node_modules/.lock-blob" 2>/dev/null)" != "$lock" ]; then
  (cd "$mumcord" && npm ci --silent --no-audit --no-fund)
  echo "$lock" > "$mumcord/node_modules/.lock-blob"
fi

# The specs run as part of Mumcord's suite: e2e/specs/official-<plugin>/ sits next to Mumcord's own
# spec folders, so `../../helpers/...` resolves to its helpers.
rm -rf "$mumcord"/e2e/specs/official-*
mkdir -p "$mumcord/e2e/specs/official-shared"
cp "$repo"/e2e/helpers/*.ts "$mumcord/e2e/specs/official-shared/"
specs=()
for id in "${plugins[@]}"; do
  mkdir -p "$mumcord/e2e/specs/official-$id"
  # Spec files and the plugin's own helper modules (any other .ts file).
  cp "$repo/$id"/e2e/*.ts "$mumcord/e2e/specs/official-$id/"
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
  # Newer Mumcord harnesses route staged packages by this list; empty means all official.
  : > "$packages/staged/routes.txt"
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
