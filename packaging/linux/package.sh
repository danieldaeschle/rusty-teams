#!/usr/bin/env bash
# Build and package Rusty Teams for Linux (<arch> is x86_64 or aarch64):
#
#   $DIST/rusty-teams-<version>-linux-<arch>.AppImage        any distro with glibc >= the build host's
#   $DIST/rusty-teams-<version>-linux-<arch>.AppImage.zsync  delta updates (needs zsyncmake)
#   $DIST/rusty-teams-<version>-linux-<arch>.deb             Debian, Ubuntu, Mint, Pop!_OS, ...
#   $DIST/rusty-teams-<version>-linux-<arch>.rpm             Fedora, openSUSE, RHEL, ...
#   $DIST/rusty-teams-<version>-linux-<arch>.pkg.tar.zst     Arch Linux
#   $DIST/rusty-teams-<version>-linux-<arch>.tar.gz          plain FHS-style tree (bin/, share/)
#
# Usage: packaging/linux/package.sh [--skip-build] [--formats "appimage deb rpm archlinux tar"]
#
# Needs: cargo and the Linux build setup from the README (clang 21+, mold); nfpm for deb/rpm/Arch
# (https://nfpm.goreleaser.com); objdump (binutils). AppImage tooling is downloaded at verified
# pins into $CARGO_TARGET_DIR. Build on an old distro (CI: Ubuntu 22.04, glibc 2.35) so the
# binary runs on newer ones. Optional: desktop-file-validate, appstreamcli, zsyncmake (the zsync
# package) for the AppImage's .zsync. --skip-build packages $CARGO_TARGET_DIR/release/teams as is.
set -euo pipefail
# Packaged files and directories get 644/755 whatever the caller's umask.
umask 022
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/linux"
# shellcheck source=verified-download.sh
. "$HERE/verified-download.sh"
# The window's app id, the .desktop file and the icon name (DESKTOP_ID in linux_desktop.rs).
DESKTOP_ID=rusty-teams
# The AppStream component id (packaging/linux/rusty-teams.metainfo.xml.in).
COMPONENT_ID=io.github.danieldaeschle.RustyTeams

SKIP_BUILD=0
FORMATS="appimage deb rpm archlinux tar"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --formats) FORMATS="$2"; shift 2 ;;
    -h | --help) sed -n '2,17p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) DEB_ARCH=amd64 ;;
  aarch64 | arm64) ARCH=aarch64; DEB_ARCH=arm64 ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac
export RUSTY_TEAMS_MAINTAINER="${RUSTY_TEAMS_MAINTAINER:-Daniel Däschle <30751516+danieldaeschle@users.noreply.github.com>}"
BASENAME="rusty-teams-$VERSION-linux-$ARCH"

echo "==> Rusty Teams $VERSION for Linux $ARCH ($FORMATS)"

if [[ "$SKIP_BUILD" = 0 ]]; then
  (cd "$ROOT" && cargo build --release --locked -p app)
fi
BIN="$CARGO_TARGET_DIR/release"
[[ -x "$BIN/teams" ]] || { echo "error: $BIN/teams not found (build it or drop --skip-build)" >&2; exit 1; }
WORK="$CARGO_TARGET_DIR/linux-package"
STAGE="$WORK/root"
rm -rf "$WORK"

# ---- stage an FHS tree (shared by every format) -------------------------------------------------
install -Dm755 "$BIN/teams" "$STAGE/usr/bin/teams"
strip "$STAGE/usr/bin/teams" 2>/dev/null || true
install -Dm644 "$HERE/$DESKTOP_ID.desktop" "$STAGE/usr/share/applications/$DESKTOP_ID.desktop"
mkdir -p "$STAGE/usr/share/metainfo"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$BUILD_DATE/g" \
  "$HERE/$DESKTOP_ID.metainfo.xml.in" >"$STAGE/usr/share/metainfo/$COMPONENT_ID.metainfo.xml"
mkdir -p "$STAGE/usr/share/icons"
cp -R "$HERE/hicolor" "$STAGE/usr/share/icons/"
mkdir -p "$STAGE/usr/share/doc/rusty-teams"
copy_docs "$STAGE/usr/share/doc/rusty-teams"

if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$STAGE/usr/share/applications/$DESKTOP_ID.desktop"
fi
if command -v appstreamcli >/dev/null; then
  appstreamcli validate --no-net --explain "$STAGE/usr/share/metainfo/$COMPONENT_ID.metainfo.xml"
fi

# The newest glibc symbol version the binary needs: the deb/rpm depend on it, and it is the
# oldest distro the AppImage and tarball run on.
GLIBC_MIN="$(objdump -T "$STAGE/usr/bin/teams" | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/^GLIBC_//' | sort -uV | tail -n 1)"
[[ -n "$GLIBC_MIN" ]] || { echo "error: could not read the glibc version from $STAGE/usr/bin/teams" >&2; exit 1; }
echo "needs glibc >= $GLIBC_MIN"

has() { case " $FORMATS " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

# ---- .tar.gz ------------------------------------------------------------------------------------
if has tar; then
  mkdir -p "$WORK/tar"
  cp -R "$STAGE/usr" "$WORK/tar/$BASENAME"
  tar -C "$WORK/tar" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
  echo "wrote $DIST/$BASENAME.tar.gz"
fi

# ---- .deb / .rpm / Arch -------------------------------------------------------------------------
if has deb || has rpm || has archlinux; then
  command -v nfpm >/dev/null || { echo "error: nfpm not found (https://nfpm.goreleaser.com/install/)" >&2; exit 1; }
  export VERSION
  export NFPM_ARCH="$DEB_ARCH"
  # pacman names the architecture x86_64/aarch64, not the Debian amd64/arm64.
  export NFPM_PACMAN_ARCH="$ARCH"
  # nfpm expands env vars in fields like `version` and `arch`, but not in `contents[].src` or
  # the dependency lists.
  sed -e "s|\${STAGE}|$STAGE|g" -e "s|\${GLIBC_MIN}|$GLIBC_MIN|g" "$HERE/nfpm.yaml" >"$WORK/nfpm.yaml"
  for fmt in deb rpm; do
    if has "$fmt"; then (cd "$ROOT" && nfpm package -f "$WORK/nfpm.yaml" -p "$fmt" -t "$DIST/$BASENAME.$fmt"); fi
  done
  if has archlinux; then
    (cd "$ROOT" && nfpm package -f "$WORK/nfpm.yaml" -p archlinux -t "$DIST/$BASENAME.pkg.tar.zst")
  fi
fi

# ---- AppImage -----------------------------------------------------------------------------------
if has appimage; then
  APPDIR="$WORK/RustyTeams.AppDir"
  cp -R "$STAGE" "$APPDIR"
  # AppRun is a script, not a symlink: it installs the .desktop entry and icons into
  # $XDG_DATA_HOME before launching, so Wayland compositors can resolve the window's
  # app_id to our icon instead of the generic Wayland logo (opt-out env in the script
  # header). Logic is covered by packaging/linux/apprun-test.sh.
  install -Dm755 "$HERE/AppRun" "$APPDIR/AppRun"
  cp "$HERE/$DESKTOP_ID.desktop" "$APPDIR/$DESKTOP_ID.desktop"
  cp "$HERE/hicolor/256x256/apps/$DESKTOP_ID.png" "$APPDIR/$DESKTOP_ID.png"
  ln -s "$DESKTOP_ID.png" "$APPDIR/.DirIcon"

  # Digests from the official GitHub release asset metadata:
  # https://github.com/AppImage/appimagetool/releases/tag/1.9.1
  # https://github.com/AppImage/type2-runtime/releases/tag/20251108
  # Keep both pins together:
  # appimagetool otherwise downloads the mutable latest type2 runtime itself.
  TOOL_VERSION=1.9.1
  RUNTIME_VERSION=20251108
  case "$ARCH" in
    x86_64)
      TOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
      RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
      ;;
    aarch64)
      TOOL_SHA256=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158
      RUNTIME_SHA256=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444
      ;;
  esac
  mkdir -p "$CARGO_TARGET_DIR"
  TOOL="${APPIMAGETOOL:-$CARGO_TARGET_DIR/appimagetool-$TOOL_VERSION-$ARCH.AppImage}"
  if [[ -n "${APPIMAGETOOL:-}" ]]; then
    verify_appimagetool_override "$TOOL" "$TOOL_SHA256" "$TOOL_VERSION" "$ARCH"
  else
    download_verified \
      "https://github.com/AppImage/appimagetool/releases/download/$TOOL_VERSION/appimagetool-$ARCH.AppImage" \
      "$TOOL" "$TOOL_SHA256" true
  fi
  RUNTIME="$CARGO_TARGET_DIR/type2-runtime-$RUNTIME_VERSION-$ARCH"
  download_verified \
    "https://github.com/AppImage/type2-runtime/releases/download/$RUNTIME_VERSION/runtime-$ARCH" \
    "$RUNTIME" "$RUNTIME_SHA256"
  # Absolute, because appimagetool runs in $DIST below (CARGO_TARGET_DIR or APPIMAGETOOL may be
  # relative).
  OUT="$(cd "$DIST" && pwd)/$BASENAME.AppImage"
  APPDIR="$(cd "$APPDIR" && pwd)"
  TOOL="$(cd "$(dirname "$TOOL")" && pwd)/$(basename "$TOOL")"
  RUNTIME="$(cd "$(dirname "$RUNTIME")" && pwd)/$(basename "$RUNTIME")"
  # A .zsync left by an earlier run would hide a missing zsyncmake and describe another file.
  rm -f "$OUT.zsync"
  # Update information: AppImageUpdate, AppImageLauncher and the like read it from the file and
  # fetch only the blocks that changed in a newer release, through the .zsync published next to
  # each AppImage on GitHub Releases. `latest` is the newest published release that is not a
  # pre-release. A fork's builds point at its own releases through GITHUB_REPOSITORY.
  REPO="${GITHUB_REPOSITORY:-danieldaeschle/rusty-teams}"
  UPDATE_INFO="gh-releases-zsync|${REPO%%/*}|${REPO#*/}|latest|rusty-teams-*-linux-$ARCH.AppImage.zsync"
  # Extract-and-run: works without FUSE (containers, CI). The output embeds the static runtime,
  # so users don't need libfuse2 either. With zsyncmake on the host appimagetool also writes the
  # .zsync, into its working directory, hence the cd.
  (cd "$DIST" && ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream \
    --runtime-file "$RUNTIME" -u "$UPDATE_INFO" "$APPDIR" "$OUT")
  echo "wrote $OUT"
  if [[ -s "$OUT.zsync" ]]; then
    echo "wrote $OUT.zsync"
  else
    warn "zsyncmake not found, so $OUT.zsync was not written; AppImage delta updates need it"
  fi
fi

echo "==> done"
ls -lh "$DIST"
