#!/usr/bin/env bash
# One Cargo target dir for every worktree, and a disk cap for it.
# Usage:
#   scripts/build-cache.sh setup           point ~/.cargo/config.toml at the shared target dir (once per machine)
#   scripts/build-cache.sh status          sizes: shared target dir, old per-worktree target dirs, kache
#   scripts/build-cache.sh clean [--apply] remove old per-worktree target dirs and trim the shared one to the cap
# Without --apply, clean only prints what it would remove.
# Env: RUSTY_TEAMS_TARGET_CAP_GB (default 40), KACHE_MAX_SIZE for `kache gc` (default 15GiB).
set -euo pipefail
repoRoot="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
sharedTarget="${XDG_CACHE_HOME:-$HOME/.cache}/rusty-teams/target"
capKilobytes=$(( ${RUSTY_TEAMS_TARGET_CAP_GB:-40} * 1024 * 1024 ))
windowsRelease="$sharedTarget/x86_64-pc-windows-msvc/release"

sizeKilobytes() { du -sk "$1" 2>/dev/null | cut -f1 || echo 0; }
human() { du -sh "$1" 2>/dev/null | cut -f1; }

# Target dirs from before the shared one: one per worktree under ~/.cache, plus target/ and crates/app/target/ in each worktree.
legacyTargets() {
  for directory in "${XDG_CACHE_HOME:-$HOME/.cache}"/cargo-target-*; do
    if [ -d "$directory" ]; then echo "$directory"; fi
  done
  git -C "$repoRoot" worktree list --porcelain | sed -n 's/^worktree //p' | while read -r worktree; do
    for directory in "$worktree/target" "$worktree/crates/app/target" "$worktree/tools/onnxruntime-dll/target" "$worktree"/spikes/*/target; do
      if [ -d "$directory" ]; then echo "$directory"; fi
    done
  done
  return 0
}

case "${1:-status}" in
  setup)
    config="$HOME/.cargo/config.toml"
    mkdir -p "$(dirname "$config")" "$sharedTarget"
    if grep -qs '^target-dir' "$config"; then
      echo "$config already sets target-dir:"; grep '^target-dir' "$config"
    elif grep -qs '^\[build\]' "$config"; then
      sed -i "/^\[build\]/a target-dir = \"$sharedTarget\"" "$config"
    else
      printf '\n[build]\ntarget-dir = "%s"\n' "$sharedTarget" >> "$config"
    fi
    echo "shared target dir: $sharedTarget"
    [ -z "${CARGO_TARGET_DIR:-}" ] || echo "CARGO_TARGET_DIR=$CARGO_TARGET_DIR is set and wins over the config; unset it (shell rc, worktree env)" >&2
    exit 0
    ;;
  status)
    echo "shared target: $(human "$sharedTarget") $sharedTarget"
    [ -d "$sharedTarget" ] && for directory in "$sharedTarget"/*/ "$sharedTarget"/x86_64-pc-windows-msvc/*/; do
      [ -d "$directory" ] && echo "  $(human "$directory") $directory"
    done
    echo "old per-worktree target dirs:"
    legacyTargets | while read -r directory; do echo "  $(human "$directory") $directory"; done
    if command -v kache >/dev/null; then echo "kache:"; kache stats 2>/dev/null | sed 's/^/  /' || true; fi
    exit 0
    ;;
  clean) ;;
  *) sed -n '2,9p' "$0"; exit 2 ;;
esac

apply=false
[ "${2:-}" = "--apply" ] && apply=true
remove() {
  echo "remove $(human "$1") $1"
  if $apply; then rm -rf "$1"; fi
}

legacyTargets | while read -r directory; do
  [ "$(realpath "$directory")" = "$(realpath -m "$sharedTarget")" ] || remove "$directory"
done

# Over the cap: drop what rebuilds fastest first. Incremental caches, then the Linux debug build, then the Windows release
# dependencies. The Windows exe itself stays, so scripts/deploy-windows.sh still works after a clean.
if [ -d "$sharedTarget" ]; then
  remainingKilobytes=$(sizeKilobytes "$sharedTarget")
  for candidate in \
    "$sharedTarget"/debug/incremental "$windowsRelease"/incremental "$sharedTarget"/release/incremental \
    "$sharedTarget"/debug "$sharedTarget"/release "$sharedTarget"/x86_64-pc-windows-msvc/debug \
    "$windowsRelease"/examples "$windowsRelease"/deps "$windowsRelease"/build; do
    [ "$remainingKilobytes" -le "$capKilobytes" ] && break
    [ -d "$candidate" ] || continue
    remainingKilobytes=$(( remainingKilobytes - $(sizeKilobytes "$candidate") ))
    remove "$candidate"
  done
  echo "shared target: $(human "$sharedTarget"), cap ${RUSTY_TEAMS_TARGET_CAP_GB:-40}G"
fi

if command -v kache >/dev/null; then
  echo "kache gc down to ${KACHE_MAX_SIZE:-15GiB}"
  if $apply; then KACHE_MAX_SIZE="${KACHE_MAX_SIZE:-15GiB}" kache gc; fi
fi
$apply || echo "dry run; add --apply to remove"
