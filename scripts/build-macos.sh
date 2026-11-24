#!/usr/bin/env bash
# Build a universal macOS binary (arm64 + x86_64) from a crate. Needs Xcode CLT only.
# Wraps it in a .app bundle (icon, Info.plist) and zips that for distribution.
# The published artifact is the `.app` zip. There is no in-app Mach-O swap.
#
# Optional signing / notarization when Apple credentials are present:
#   MACOS_CODESIGN_IDENTITY   e.g. "Developer ID Application: …"
#   MACOS_NOTARY_PROFILE      notarytool keychain profile (from `xcrun notarytool store-credentials`)
# Without those env vars the .app is ad-hoc signed (fine for local / internal RC).
#
# Usage: scripts/build-macos.sh [crate-dir, default crates/app] [binary-or-example-name, default: the crate's only binary]
set -euo pipefail
repoRoot="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
crateDir="$(realpath "${1:-$repoRoot/crates/app}")"
binaryName="${2:-}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$repoRoot/target}"
export PATH="$HOME/.cargo/bin:$PATH"
entitlements="$repoRoot/scripts/macos/RustyTeams.entitlements"
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

# Developer ID signing when a certificate is configured in the build environment;
# otherwise ad-hoc sign the bundle so both universal slices share a valid
# CodeDirectory bound to Info.plist (CFBundleIdentifier) instead of leaving
# arm64 linker-signed and x86_64 unsigned (which breaks TCC matching on Apple Silicon).
if [ -n "${MACOS_CODESIGN_IDENTITY:-}" ]; then
  echo "codesign with $MACOS_CODESIGN_IDENTITY"
  codesign --force --options runtime --timestamp \
    --entitlements "$entitlements" \
    --sign "$MACOS_CODESIGN_IDENTITY" \
    "$app/Contents/MacOS/$binary"
  codesign --force --options runtime --timestamp \
    --entitlements "$entitlements" \
    --sign "$MACOS_CODESIGN_IDENTITY" \
    "$app"
  codesign --verify --deep --strict --verbose=2 "$app"
else
  echo "codesign ad-hoc (-)"
  codesign --force --sign - "$app"
  codesign --verify --deep --strict --verbose=2 "$app"
fi

# ditto keeps the bundle intact (plain zip would not); the result is the .app release asset.
ditto -c -k --keepParent "$app" "$outputDirectory/teams-macos.zip"

if [ -n "${MACOS_CODESIGN_IDENTITY:-}" ] && [ -n "${MACOS_NOTARY_PROFILE:-}" ]; then
  echo "notarize with profile $MACOS_NOTARY_PROFILE"
  xcrun notarytool submit "$outputDirectory/teams-macos.zip" \
    --keychain-profile "$MACOS_NOTARY_PROFILE" \
    --wait
  xcrun stapler staple "$app"
  # Re-zip after staple so the published .app zip carries the ticket.
  ditto -c -k --keepParent "$app" "$outputDirectory/teams-macos.zip"
fi

file "$output"
ls -la "$output" "$app/Contents/MacOS/$binary" \
  "$outputDirectory/teams-macos.zip"
