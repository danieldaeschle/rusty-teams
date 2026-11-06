#!/usr/bin/env bash
# Build a universal macOS binary (arm64 + x86_64) from a crate. Needs Xcode CLT only.
# Wraps it in a .app bundle (icon, Info.plist) and zips that for distribution.
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

# Wrap the binary in a .app so Finder and the Dock treat it as an application:
# double-clickable, with the icon, and with the camera / microphone / screen
# permission prompts (macOS keys those off the bundle, not a bare binary).
app="$outputDirectory/Rusty Teams.app"
binary="$(basename "$output")"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$repoRoot/Cargo.toml" | head -1)"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$output" "$app/Contents/MacOS/$binary"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Rusty Teams</string>
    <key>CFBundleDisplayName</key>
    <string>Rusty Teams</string>
    <key>CFBundleIdentifier</key>
    <string>org.rusty-teams.teams</string>
    <key>CFBundleExecutable</key>
    <string>$binary</string>
    <key>CFBundleIconFile</key>
    <string>icon</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$version</string>
    <key>CFBundleVersion</key>
    <string>$version</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSMicrophoneUsageDescription</key>
    <string>Rusty Teams uses your microphone for calls.</string>
    <key>NSCameraUsageDescription</key>
    <string>Rusty Teams uses your camera for video calls.</string>
    <key>NSScreenCaptureUsageDescription</key>
    <string>Rusty Teams shares your screen in meetings.</string>
</dict>
</plist>
PLIST

# The same icon the Windows exe embeds (assets/icon/app.rc); iconutil compiles
# whatever sizes the set has, all downscaled from the 256px original.
iconset="$(mktemp -d)/teams.iconset"
mkdir -p "$iconset"
png="$repoRoot/crates/app/assets/icon/teams-fast-256.png"
for size in 16 32 64 128 256; do
  sips -z "$size" "$size" "$png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
done
cp "$iconset/icon_32x32.png" "$iconset/icon_16x16@2x.png"
cp "$iconset/icon_64x64.png" "$iconset/icon_32x32@2x.png"
cp "$iconset/icon_256x256.png" "$iconset/icon_128x128@2x.png"
iconutil -c icns "$iconset" -o "$app/Contents/Resources/icon.icns"

# ditto keeps the bundle intact (plain zip would not); the result is the release asset.
ditto -c -k --keepParent "$app" "$outputDirectory/teams-macos.zip"

file "$output"
ls -la "$output" "$app/Contents/MacOS/$binary" "$outputDirectory/teams-macos.zip"
