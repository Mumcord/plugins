#!/usr/bin/env bash
# Rebuilds Skyloom's sound effects in ../sounds/ from formulas, so they are original and
# reproducible. Needs ffmpeg with libvorbis. Short and soft on purpose: nothing during weaving is
# longer than 300 ms, and Mumcord ducks plugin sounds while anyone talks.
set -euo pipefail
out="$(cd "$(dirname "${BASH_SOURCE[0]}")/../sounds" && pwd)"

make() {
  ffmpeg -nostdin -hide_banner -loglevel error -y -f lavfi -i "aevalsrc=exprs='$2':s=44100" \
    -t "$3" -ac 1 -c:a libvorbis -q:a 3 -map_metadata -1 -fflags +bitexact -flags:a +bitexact \
    "$out/$1.ogg"
}

# A harp-like pluck when a star settles; higher on the loom, higher note (C5 E5 G5 A5 C6).
i=1
for f in 523.25 659.25 783.99 880.00 1046.50; do
  make "settle-$i" "0.5*exp(-9*t)*(sin(2*PI*$f*t)+0.35*sin(4*PI*$f*t)+0.12*sin(6*PI*$f*t))*min(1,t*400)" 0.28
  i=$((i + 1))
done
# A low shimmer when your star is plucked.
make shimmer "0.25*exp(-7*t)*(sin(2*PI*1568*t)+sin(2*PI*1975.5*t)+0.6*sin(2*PI*2637*t))*(0.6+0.4*sin(2*PI*22*t))*min(1,t*200)" 0.3
# The rising "thread pull" at dawn.
make thread-pull "0.35*sin(2*PI*(220*t+260*t*t))*min(1,t*8)*exp(-1.2*t)*(1-t/1.3)" 1.3
# A quiet chime when a constellation is woven.
make chime "0.4*exp(-3.2*t)*(sin(2*PI*1318.5*t)+0.5*sin(2*PI*1975.5*t)+0.3*sin(2*PI*2637*t)+0.2*sin(2*PI*3951*t))*min(1,t*300)" 1.0
