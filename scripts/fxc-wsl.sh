#!/usr/bin/env bash
# fxc.exe stand-in for WSL: translates paths, runs fxc-shim.ps1 on Windows. Point GPUI_FXC_PATH here.
set -euo pipefail
shimDir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
powershellExe=/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe
# WSL interop calls time out under load ("UtilAcceptVsock: accept4 failed 110"): retry each one.
interop() {
  local output attempt
  for attempt in 1 2 3; do
    if output="$("$@" 2>&1 | tr -d '\0')"; then
      printf '%s' "$output"
      return 0
    fi
    sleep "$attempt"
  done
  printf 'fxc-wsl: %s failed 3 times:\n%s\n' "$1" "$output" >&2
  return 1
}
# UNC paths break D3DCompileFromFile includes: stage on the Windows drive.
windowsProfile="$(cd /mnt/c && interop /mnt/c/Windows/System32/cmd.exe /c "echo %USERPROFILE%" | tr -d '\r')"
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
interop "$powershellExe" -NoProfile -ExecutionPolicy Bypass -File "$(wslpath -w "$shimDir/fxc-shim.ps1")" "${translatedArgs[@]}" >/dev/null
cp "$stageDir/out.h" "$headerOut"
