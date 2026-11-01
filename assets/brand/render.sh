#!/usr/bin/env bash
# Renders every raster asset from the SVG masters: the repository banner and icon in this
# folder, and each official plugin's icon at 512, 128 and 32 px next to its icon.svg.
# Needs rsvg-convert (librsvg). Run it after changing a master.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)

rsvg-convert -w 1280 -h 640 "$here/plugins-banner.svg" -o "$here/plugins-banner.png"
rsvg-convert -w 512 -h 512 "$here/plugins-icon.svg" -o "$here/plugins-icon.png"

for plugin in poll dice echolings mumbot-music eve-fit; do
    for size in 512 128 32; do
        rsvg-convert -w "$size" -h "$size" "$root/$plugin/assets/icon.svg" -o "$root/$plugin/assets/icon-$size.png"
    done
done
