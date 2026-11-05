#!/usr/bin/env bash
# Build a universal macOS binary (arm64 + x86_64) from a crate. Needs Xcode CLT only.
# Usage: scripts/build-macos.sh [crate-dir, default crates/app] [binary-or-example-name, default: the crate's only binary]
set -euo pipefail
repoRoot="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
crateDir="$(realpath "${1:-$repoRoot/crates/app}")"
binaryName="${2:-}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$repoRoot/target}"
export PATH="$HOME/.cargo/bin:$PATH"
targets=(aarch64-apple-darwin x86_64-apple-darwin)

rustup target add "${targets[@]}" >/dev/null

buildArguments=(build --release)
slicePathInTarget="release"
if [ -n "$binaryName" ] && [ -f "$crateDir/examples/$binaryName.rs" ]; then
  buildArguments+=(--example "$binaryName")
  slicePathInTarget="release/examples"
elif [ -n "$binaryName" ]; then
  buildArguments+=(--bin "$binaryName")
fi

for target in "${targets[@]}"; do
  (cd "$crateDir" && cargo "${buildArguments[@]}" --target "$target")
done

slices=()
for target in "${targets[@]}"; do
  sliceDirectory="$CARGO_TARGET_DIR/$target/$slicePathInTarget"
  if [ -n "$binaryName" ]; then
    slices+=("$sliceDirectory/$binaryName")
  else
    candidates=("$sliceDirectory"/*)
    [ "${#candidates[@]}" -eq 1 ] && [ -f "${candidates[0]}" ] ||
      { echo "more than one binary in $sliceDirectory, pass the name" >&2; exit 1; }
    slices+=("${candidates[0]}")
  fi
done

outputDirectory="$CARGO_TARGET_DIR/universal/release"
mkdir -p "$outputDirectory"
output="$outputDirectory/$(basename "${slices[0]}")"
lipo -create -output "$output" "${slices[@]}"
file "$output"
ls -la "$output"
