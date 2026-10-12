#!/usr/bin/env bash
# Usage: scripts/layout-sweep.sh [OUT_DIR]
# Screenshots every main screen at the minimum and a large window size. Run `cargo build -p app` first.
# Output: OUT_DIR/<size>-<screen>.png (default target/layout-sweep)
set -euo pipefail

out_dir=${1:-target/layout-sweep}
script_dir=$(cd "$(dirname "$0")" && pwd)
sizes=(640x480 1920x1080)
status_avatar_offset_from_right=${STATUS_AVATAR_OFFSET:-34}
status_avatar_y=20
mkdir -p "$out_dir"

for window_size in "${sizes[@]}"; do
  width=${window_size%x*}
  height=${window_size#*x}
  screen_size="$((width + 480))x$((height > 900 ? height : 900))"
  focus=("mousemove $((width / 2)) 3" "click 1" "sleep 1.5")
  captured=()
  capture() {
    local screen=$1
    local target="$out_dir/$window_size-$screen.png"
    shift
    for attempt in 1 2 3; do
      "$script_dir/headless-shot.sh" "$target" --size "$screen_size" --window "$window_size" --wait 15 --settle 3 -- "${focus[@]}" "$@" >/dev/null
      local checksum
      checksum=$(convert "$target" -crop "${width}x100+0+0" +repage png:- | md5sum)
      [[ " ${captured[*]:-} " != *" $checksum "* ]] && break
      echo "$target repeats an earlier screen, input dropped (attempt $attempt)" >&2
    done
    captured+=("$checksum")
  }
  capture chats
  capture activity "key ctrl+1" "key ctrl+1"
  capture channels "key ctrl+3" "key ctrl+3"
  capture saved "key ctrl+4" "key ctrl+4"
  capture calls "key ctrl+5" "key ctrl+5"
  capture status-menu "mousemove $((width - status_avatar_offset_from_right)) $status_avatar_y" "click 1"
  capture switcher "key ctrl+k"
  capture shortcuts "key ctrl+period"
done

ls "$out_dir"/*.png
duplicates=$(for target in "$out_dir"/*.png; do
  width=$(identify -format %w "$target")
  echo "$(convert "$target" -crop "${width}x100+0+0" +repage png:- | md5sum | cut -c1-32) $target"
done | sort | uniq -D -w32)
[[ -z $duplicates ]] || { echo "identical screenshots, an input was dropped, rerun:" >&2; echo "$duplicates" >&2; exit 1; }
