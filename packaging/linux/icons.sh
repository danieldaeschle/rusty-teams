#!/usr/bin/env bash
# Regenerate the Linux hicolor theme icons in packaging/linux/hicolor from the app icon
# (crates/app/assets/icon/teams-fast-256.png, the one the app writes when it installs
# itself, see AppIcon::RustyTeams in crates/app-base/src/appearance.rs).
#
# Needs ImageMagick (magick or convert). The outputs are committed, so packaging never
# needs it.
#
#   packaging/linux/icons.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$ROOT/crates/app/assets/icon/teams-fast-256.png"
OUT="$ROOT/packaging/linux/hicolor"
NAME=rusty-teams

if command -v magick >/dev/null; then IM=(magick); elif command -v convert >/dev/null; then IM=(convert); else
  echo "error: ImageMagick not found (apt install imagemagick)" >&2
  exit 1
fi

rm -rf "$OUT"
for s in 16 24 32 48 64 128 256; do
  mkdir -p "$OUT/${s}x${s}/apps"
  # -strip drops timestamps and profiles, so a rerun on the same source is byte-identical.
  "${IM[@]}" "$SRC" -filter Lanczos -resize "${s}x${s}" -strip "PNG32:$OUT/${s}x${s}/apps/$NAME.png"
done
echo "icons written to $OUT"
