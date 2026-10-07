#!/usr/bin/env bash
# Cross-build a GPUI crate to a Windows .exe from WSL/Linux. No admin, no Visual Studio.
# Usage: scripts/build-windows.sh [crate-dir, default spikes/gpui-list] [binary-name, default: the crate's only binary]
set -euo pipefail
repoRoot="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
crateDir="$(realpath "${1:-$repoRoot/spikes/gpui-list}")"
binaryName="${2:-}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$crateDir/target}"
export PATH="$HOME/.cargo/bin:$PATH"
export XWIN_ACCEPT_LICENSE=1
export GPUI_FXC_PATH="$repoRoot/scripts/fxc-wsl.sh"
targetName=x86_64-pc-windows-msvc

# embed-resource looks for plain `llvm-rc`, cc-rs (sqlite) for plain `llvm-lib`; Debian/Ubuntu only ship <tool>-<ver>.
toolShimDir="$(mktemp -d)"; trap 'rm -rf "$toolShimDir"' EXIT
for tool in llvm-rc llvm-lib; do
  for candidate in /usr/lib/llvm-*/bin/$tool /usr/bin/$tool-*; do
    [ -x "$candidate" ] && ln -sf "$candidate" "$toolShimDir/$tool" && break
  done
done
export PATH="$toolShimDir:$PATH"

command -v cargo-xwin >/dev/null || cargo install cargo-xwin --locked
rustup target add "$targetName"

# gpui-pre-windows/build.rs gates shader compilation on the HOST os. Patch a copy to gate on the target.
patchedDir="$CARGO_TARGET_DIR/patched/gpui-pre-windows"
sourceDir="$(cd "$crateDir" && cargo metadata --format-version 1 --filter-platform "$targetName" |
  python3 -c 'import json,sys; print(next(p["manifest_path"] for p in json.load(sys.stdin)["packages"] if p["name"]=="gpui-pre-windows"))' | xargs dirname)"
rm -rf "$patchedDir"; mkdir -p "$(dirname "$patchedDir")"; cp -r "$sourceDir" "$patchedDir"; chmod -R u+w "$patchedDir"
python3 - "$patchedDir/build.rs" <<'PY'
import re, sys
path = sys.argv[1]
text = open(path).read()
text = text.replace('#[cfg(all(target_os = "windows", not(debug_assertions)))]', '#[cfg(not(debug_assertions))]')
text = re.sub(r'fn main\(\) \{.*?\n\}\n', 'fn main() {\n    #[cfg(not(debug_assertions))]\n    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {\n        compile_shaders();\n    }\n}\n', text, count=1, flags=re.S)
text = re.sub(r'(pub fn find_latest_windows_sdk_binary\(.*?\) -> Result<[^{]*\{).*?(\n    /// You can set)', r'\1\n        let _ = binary;\n        Ok(None)\n    }\n\2', text, count=1, flags=re.S)
open(path, 'w').write(text)
PY

cd "$crateDir"
binaryArguments=()
[ -n "$binaryName" ] && binaryArguments=(--bin "$binaryName")
cargo xwin build --release --target "$targetName" "${binaryArguments[@]}" \
  --config "patch.crates-io.gpui-pre-windows.path=\"$patchedDir\""
ls -la "$CARGO_TARGET_DIR/$targetName/release/"*.exe
