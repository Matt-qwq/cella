#!/usr/bin/env bash
# Regenerate every app icon from assets/app-icon/cella.svg.
#
# Needs: resvg (brew install resvg / cargo install resvg). On macOS, iconutil also writes the
# .icns. The outputs are committed, so builds and packaging never need these tools.
#
#   packaging/icons.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="$ROOT/assets/app-icon"
SVG="$DIR/cella.svg"
# The same geometry with the grain dropped and the contrast raised: a 0.85-frequency grain reads as
# dirt at 16 px, and a blurred shadow merges with the plate. Sizes up to 48 bake from this file.
SMALL="$DIR/cella-small.svg"
ID="io.github.matt-qwq.cella"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

command -v resvg >/dev/null || { echo "error: resvg not found (brew install resvg)" >&2; exit 1; }

# The artwork is a full-bleed 512 tile (rx=112). Windows and Linux use it as is. macOS icons sit on
# Apple's grid: an 824/1024 body with a transparent margin, so widen the viewBox to 512*1024/824.
MAC="$TMP/mac.svg"
MAC_SMALL="$TMP/mac-small.svg"
mac_viewbox() { sed 's/viewBox="0 0 512 512"/viewBox="-62.136 -62.136 636.272 636.272"/' "$1" >"$2"; }
mac_viewbox "$SVG" "$MAC"
mac_viewbox "$SMALL" "$MAC_SMALL"
grep -q 'viewBox="-62.136' "$MAC" || { echo "error: expected viewBox=\"0 0 512 512\" in $SVG" >&2; exit 1; }

render() { resvg -w "$2" -h "$2" "$1" "$3" </dev/null; }
# The art for a size: the small variant up to 48 px, the full one above it.
art() { if [ "$1" -le 48 ]; then echo "$SMALL"; else echo "$SVG"; fi; }

render "$SVG" 1024 "$DIR/cella-1024.png"
# Runtime window/Dock icon on macOS (for apps/cella to embed); other platforms use hicolor 256.
render "$MAC" 512 "$DIR/cella-macos-512.png"

# Linux hicolor theme.
for s in 16 24 32 48 64 128 256 512; do
  mkdir -p "$DIR/hicolor/${s}x${s}/apps"
  render "$(art "$s")" "$s" "$DIR/hicolor/${s}x${s}/apps/$ID.png"
done
mkdir -p "$DIR/hicolor/scalable/apps"
cp "$SVG" "$DIR/hicolor/scalable/apps/$ID.svg"

# Windows .ico.
ICO_PNGS=()
for s in 16 20 24 32 40 48 64 128 256; do
  render "$(art "$s")" "$s" "$TMP/ico-$s.png"
  ICO_PNGS+=("$TMP/ico-$s.png")
done
(cd "$ROOT" && cargo run -q -p xtask --no-default-features -- ico "$DIR/cella.ico" "${ICO_PNGS[@]}")

# macOS .icns.
if command -v iconutil >/dev/null; then
  SET="$TMP/cella.iconset"
  mkdir -p "$SET"
  for s in 16 32 128 256 512; do
    # 16 and 32 (and their 2x) keep the small art; the rest is the full plate.
    m="$( [ "$s" -le 32 ] && echo "$MAC_SMALL" || echo "$MAC" )"
    render "$m" "$s" "$SET/icon_${s}x${s}.png"
    render "$m" $((s * 2)) "$SET/icon_${s}x${s}@2x.png"
  done
  iconutil -c icns -o "$DIR/cella.icns" "$SET"
else
  echo "warning: iconutil not found (macOS only); cella.icns not regenerated" >&2
fi
echo "icons written to $DIR"
