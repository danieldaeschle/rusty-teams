#!/usr/bin/env bash
# Usage: scripts/headless-shot.sh OUT.png [--size 1280x800] [--wait 10] [--bin target/debug/teams] [--] [xdotool command]...
# Runs the Linux build with --demo on a private Xvfb display and saves a screenshot.
# Example: scripts/headless-shot.sh shot.png -- "mousemove 450 740" "click 1" "type hello" "paste ```\\ncode\\n```"
# `type` cannot send a backtick (X11 treats it as a dead key and input stops); use `paste`, which expands \n.
set -euo pipefail

out=${1:?output png}
shift
size=1280x800
wait_seconds=10
binary=target/debug/teams
while [[ $# -gt 0 ]]; do
  case $1 in
    --size) size=$2; shift 2 ;;
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

for command in "$@"; do
  subcommand=${command%% *}
  rest=${command#"$subcommand"}
  rest=${rest# }
  if [[ $subcommand == click ]]; then
    # Xvfb has no window manager: keyboard focus only follows an explicit windowfocus.
    eval "$(xdotool getmouselocation --shell)"
    xdotool windowfocus --sync "$WINDOW"
  fi
  if [[ $subcommand == type ]]; then
    xdotool type --delay 20 -- "$rest"
  elif [[ $subcommand == paste ]]; then
    printf '%b' "$rest" | xclip -selection clipboard
    xdotool key ctrl+v
  else
    read -r -a arguments <<<"$rest"
    xdotool "$subcommand" "${arguments[@]}"
  fi
  sleep 0.4
done
sleep 1
import -window root "$out"
echo "$out"
