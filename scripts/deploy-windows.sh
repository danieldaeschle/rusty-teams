#!/usr/bin/env bash
# Publish the release exe to %LOCALAPPDATA%\Programs\ms-teams-linux\update\. The running app swaps itself.
# Usage: scripts/deploy-windows.sh [exe, default crates/app/target/x86_64-pc-windows-msvc/release/teams.exe]
set -euo pipefail
repoRoot="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
targetDirectory="${CARGO_TARGET_DIR:-$repoRoot/crates/app/target}"
sourceExe="$(realpath "${1:-$targetDirectory/x86_64-pc-windows-msvc/release/teams.exe}")"
installRoot="${TEAMS_INSTALL_DIR:-}"
if [ -z "$installRoot" ]; then
  localAppData="$(PATH="$PATH:/mnt/c/Windows/System32" cmd.exe /c 'echo %LOCALAPPDATA%' 2>/dev/null | tr -d '\r')"
  installRoot="$(wslpath "$localAppData")/Programs/ms-teams-linux"
fi
version="$(grep -aoE 'teams-build:[0-9a-f]+-[0-9]{8}T[0-9]{6}Z' "$sourceExe" | head -n1 | cut -d: -f2)"
[ -n "$version" ] || { echo "no build version found in $sourceExe" >&2; exit 1; }

updateDirectory="$installRoot/update"
mkdir -p "$updateDirectory"
cp "$sourceExe" "$updateDirectory/teams.exe.partial"
mv -f "$updateDirectory/teams.exe.partial" "$updateDirectory/teams.exe"
printf '%s\n' "$version" > "$updateDirectory/version.txt.partial"
mv -f "$updateDirectory/version.txt.partial" "$updateDirectory/version.txt"
echo "deployed $version to $updateDirectory"
