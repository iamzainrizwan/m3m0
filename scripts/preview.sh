#!/usr/bin/env bash
# Render preview frames without disturbing the main (laptop) display.
#   scripts/preview.sh OUTDIR [M3M0_OPEN] [M3M0_KEYS]
# Runs under XWayland (no WAYLAND_DISPLAY) so the window can be placed on the
# secondary monitor (DP-1 sits at 0,0, above the laptop panel), laptop-sized
# (1706x1066 points). Last frame: OUTDIR/09.png.
set -euo pipefail
out=${1:?outdir}; mkdir -p "$out"; rm -f "$out"/*.png
cd "$(dirname "$0")/.."
env -u WAYLAND_DISPLAY M3M0_SHOTS="$out" M3M0_PREVIEW_POS="${M3M0_PREVIEW_POS:-60,60}" \
  M3M0_OPEN="${2:-}" M3M0_KEYS="${3:-}" timeout 30 ./target/release/m3m0 >/dev/null 2>&1 || true
[ -s "$out/09.png" ] || echo "preview: no frame captured (display asleep or app failed)" >&2
