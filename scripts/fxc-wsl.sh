#!/usr/bin/env bash
# fxc.exe stand-in for WSL: translates paths, runs fxc-shim.ps1 on Windows. Point GPUI_FXC_PATH here.
set -euo pipefail
shimDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
powershellExe=/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe
# UNC paths break D3DCompileFromFile includes: stage on the Windows drive.
windowsProfile="$(cd /mnt/c && /mnt/c/Windows/System32/cmd.exe /c "echo %USERPROFILE%" 2>/dev/null | tr -d '')"
stageDir="$(mktemp -d "$(wslpath "$windowsProfile")/AppData/Local/Temp/fxc-stage.XXXXXX")"
trap 'rm -rf "$stageDir"' EXIT
translatedArgs=(); headerOut=""
while (($#)); do
  case "$1" in
    /T|/E|/Vn) translatedArgs+=("$1" "$2"); shift 2 ;;
    /Fh) headerOut="$2"; translatedArgs+=("$1" "$(wslpath -w "$stageDir/out.h")"); shift 2 ;;
    /O3) translatedArgs+=("$1"); shift ;;
    *) cp "$(dirname "$1")"/*.hlsl "$stageDir/"; translatedArgs+=("$(wslpath -w "$stageDir/$(basename "$1")")"); shift ;;
  esac
done
"$powershellExe" -NoProfile -ExecutionPolicy Bypass -File "$(wslpath -w "$shimDir/fxc-shim.ps1")" "${translatedArgs[@]}"
cp "$stageDir/out.h" "$headerOut"
