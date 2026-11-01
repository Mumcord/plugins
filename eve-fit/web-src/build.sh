#!/usr/bin/env bash
# Rebuilds ../web from the exact package versions in package-lock.json (npm ci checks their
# sha512 integrity) and the ship renders pinned in renders.lock. Needs Node 24+ and network access
# to the npm registry and CCP's image server. Arguments go to scripts/build.mjs
# (--check, --update-renders).
set -euo pipefail
cd "$(dirname "$0")"
npm ci --no-audit --no-fund --loglevel=error
node scripts/build.mjs "$@"
