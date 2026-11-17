# shellcheck shell=bash
# Shared setup for the packaging scripts. Source it: `. "$(dirname "$0")/../env.sh"`.
#
# Exports:
#   ROOT              workspace root
#   VERSION           [workspace.package] version from Cargo.toml (override: RUSTY_TEAMS_VERSION)
#   DIST              output directory for release artifacts (default: $ROOT/dist/release)
#   BUILD_DATE        UTC build date, YYYY-MM-DD (override: RUSTY_TEAMS_BUILD_DATE)
#   CARGO_TARGET_DIR  cargo's target dir (default: $ROOT/target)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export ROOT

# The version lives in one place: `[workspace.package] version` in the root Cargo.toml.
workspace_version() {
  awk '
    /^\[/ { in_pkg = ($0 == "[workspace.package]") ; next }
    in_pkg && $1 == "version" { gsub(/[" ]/, "", $3); print $3; exit }
  ' "$ROOT/Cargo.toml"
}

VERSION="${RUSTY_TEAMS_VERSION:-$(workspace_version)}"
VERSION="${VERSION#v}"
if [[ -z "$VERSION" ]]; then
  echo "error: could not read [workspace.package] version from $ROOT/Cargo.toml" >&2
  exit 1
fi
export VERSION

DIST="${DIST:-$ROOT/dist/release}"
mkdir -p "$DIST"
export DIST

export BUILD_DATE="${RUSTY_TEAMS_BUILD_DATE:-$(date -u +%Y-%m-%d)}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

# Emit a GitHub Actions warning (plain stderr outside Actions).
warn() {
  if [[ -n "${GITHUB_ACTIONS:-}" ]]; then echo "::warning::$*"; else echo "warning: $*" >&2; fi
}

# Copy licence and readme files that exist into a package directory.
copy_docs() {
  local dest="$1" f
  for f in README.md LICENSE; do
    if [[ -f "$ROOT/$f" ]]; then cp "$ROOT/$f" "$dest/"; fi
  done
}

# Portable SHA-256 of a file (prints just the hash).
sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}
