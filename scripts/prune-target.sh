#!/usr/bin/env bash
# Shrink the shared cargo target dir: keep only the newest variant of each linked binary, drop old incremental sessions.
# Usage: scripts/prune-target.sh [target-dir, default $CARGO_TARGET_DIR] [incremental max age in days, default 1]
set -euo pipefail
targetDir="${1:-${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR or pass the target dir}}"
incrementalMaxAgeDays="${2:-1}"
cd "$targetDir"
sizeBefore=$(du -s --block-size=1G . | cut -f1)

for profileDir in debug release x86_64-pc-windows-msvc/debug x86_64-pc-windows-msvc/release; do
  for subDir in deps examples; do
    binaryDir="$profileDir/$subDir"
    [ -d "$binaryDir" ] || continue
    find "$binaryDir" -maxdepth 1 -type f -executable ! -name '*.so' ! -name '*.d' ! -name '*.rlib' ! -name '*.rmeta' -printf '%T@ %p\n' |
      sort -rn |
      awk '{ name = $2; sub(/-[0-9a-f]{16}(\.exe)?$/, "", name); extension = $2; sub(/.*-[0-9a-f]{16}/, "", extension); if (seen[name extension]++) print $2 }' |
      while read -r staleBinary; do
        rm -f "$staleBinary" "${staleBinary%.exe}.pdb" "$staleBinary.dwp"
      done
  done
  for incrementalDir in "$profileDir"/incremental*; do
    [ -d "$incrementalDir" ] || continue
    find "$incrementalDir" -mindepth 1 -maxdepth 1 -type d -mtime +"$incrementalMaxAgeDays" -exec rm -rf {} +
  done
done

echo "target dir: ${sizeBefore}G -> $(du -s --block-size=1G . | cut -f1)G"
