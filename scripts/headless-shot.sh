#!/usr/bin/env bash
# Usage: scripts/headless-shot.sh OUT.png [--size 1280x800] [--window WxH] [--wait 10] [--settle 1] [--bin TARGET_DIR/debug/teams] [--] [xdotool command]...
# Runs the Linux build with --demo on a private Xvfb display and saves a screenshot.
# Example: scripts/headless-shot.sh shot.png -- "mousemove 450 740" "click 1" "type hello" "paste ```\\ncode\\n```"
# `type` cannot send a backtick (X11 treats it as a dead key and input stops); use `paste`, which expands \n.
set -euo pipefail

out=${1:?output png}
shift
size=1280x800
wait_seconds=10
window_size=
settle_seconds=1
binary="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')/debug/teams"
while [[ $# -gt 0 ]]; do
  case $1 in
    --size) size=$2; shift 2 ;;
    --window) window_size=$2; shift 2 ;;
    --settle) settle_seconds=$2; shift 2 ;;
    --wait) wait_seconds=$2; shift 2 ;;
    --bin) binary=$2; shift 2 ;;
    --) shift; break ;;
    *) break ;;
  esac
done

display_number=$((100 + RANDOM % 800))
Xvfb ":$display_number" -screen 0 "${size}x24" -nolisten tcp >/dev/null 2>&1 &
xvfb_pid=$!
app_pid=
cleanup() {
  [[ -n $app_pid ]] && kill "$app_pid" 2>/dev/null || true
  kill "$xvfb_pid" 2>/dev/null || true
}
trap cleanup EXIT
sleep 1

export DISPLAY=":$display_number"
unset WAYLAND_DISPLAY
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
"$binary" --demo >/dev/null 2>&1 &
app_pid=$!
sleep "$wait_seconds"

if [[ -n $window_size ]]; then
  window_width=${window_size%x*}
  window_height=${window_size#*x}
  window_id=$(xdotool search --sync --onlyvisible --pid "$app_pid" --name '^Rusty Teams$' | head -n 1)
  resized=false
  for _ in 1 2 3 4 5 6 7 8; do
    xdotool windowmove "$window_id" 0 0
    xdotool windowsize "$window_id" "$window_width" "$window_height"
    sleep 1
    eval "$(xdotool getwindowgeometry --shell "$window_id")"
    if [[ $WIDTH == "$window_width" && $HEIGHT == "$window_height" ]]; then
      resized=true
      break
    fi
  done
  if [[ $resized != true ]]; then
    echo "window never reached ${window_size}, last geometry ${WIDTH}x${HEIGHT}" >&2
    exit 1
  fi
fi

for command in "$@"; do
  subcommand=${command%% *}
  rest=${command#"$subcommand"}
  rest=${rest# }
  if [[ $subcommand == click || $subcommand == key ]]; then
    # Xvfb has no window manager: keyboard focus only follows an explicit windowfocus.
    eval "$(xdotool getmouselocation --shell)"
    xdotool windowfocus --sync "$WINDOW"
  fi
  if [[ $subcommand == type ]]; then
    xdotool type --delay 20 -- "$rest"
  elif [[ $subcommand == key && $rest == *+* ]]; then
    # GPUI drops the key when the modifier and the key arrive in the same instant.
    modifier=${rest%%+*}
    xdotool keydown "$modifier"
    sleep 0.15
    xdotool key "${rest#*+}"
    sleep 0.15
    xdotool keyup "$modifier"
  elif [[ $subcommand == paste ]]; then
    printf '%b' "$rest" | xclip -selection clipboard
    xdotool key ctrl+v
  else
    read -r -a arguments <<<"$rest"
    xdotool "$subcommand" "${arguments[@]}"
  fi
  sleep 0.4
done
sleep "$settle_seconds"
if [[ -n $window_size ]]; then
  import -window root -crop "${window_size}+0+0" +repage "$out"
else
  import -window root "$out"
fi
echo "$out"
