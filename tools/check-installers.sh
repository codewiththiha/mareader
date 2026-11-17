#!/usr/bin/env bash
# Usage: tools/check-installers.sh OUT_DIR (cargo-packager's output).
# Installs, launches and removes this OS's installers.

# SMOKE=required fails a launch that dies. Linux needs a display (xvfb-run).
set -euo pipefail
cd "$(dirname "$0")/.."

out=${1:?usage: tools/check-installers.sh OUT_DIR}
book="$PWD/crates/epub/tests/books/the-raven-gutenberg-17192.epub"
report=${REPORT:-target/installers}
mkdir -p "$report"
report=$(cd "$report" && pwd)
failed=0

say() { printf '%s\n' "$*" | tee -a "$report/checks.txt"; }
ok() { say "ok    $*"; }
bad() { say "FAIL  $*"; failed=1; }
check() { # check "what" command...
  local what=$1; shift
  if "$@" >>"$report/log.txt" 2>&1; then ok "$what"; else bad "$what"; fi
}
one() { # the single file matching a glob, or nothing
  local hits=("$@")
  [ "${#hits[@]}" -eq 1 ] && [ -e "${hits[0]}" ] && printf '%s' "${hits[0]}"
}

# Every installer, its size and checksum, for the run's summary.
manifest() {
  : >"$report/manifest.txt"
  for f in "$out"/*.deb "$out"/*.AppImage "$out"/*.tar.gz "$out"/*.dmg \
           "$out"/*-setup.exe "$out"/*.msi; do
    [ -f "$f" ] || continue
    local sum
    sum=$( (sha256sum "$f" 2>/dev/null || shasum -a 256 "$f") | cut -d' ' -f1)
    printf '%s  %s  %s\n' "$sum" "$(du -k "$f" | cut -f1) KiB" \
      "$(basename "$f")" >>"$report/manifest.txt"
  done
  cat "$report/manifest.txt"
  [ -s "$report/manifest.txt" ] || bad "no installers in $out"
}

# Runs "$@" with a book for 12 s: alive means the window came up.
smoke() { # smoke "what" command...
  local what=$1; shift
  "$@" "$(native "$book")" >"$report/smoke-$what.log" 2>&1 &
  local pid=$!
  sleep 12
  if alive "$pid"; then
    ok "$what launches and stays up with a book"
    shot "$what"
    stop "$pid"
  elif [ "${SMOKE:-}" = required ]; then
    bad "$what exited at launch (smoke-$what.log)"
  else
    say "warn  $what exited at launch, no GPU here? (smoke-$what.log)"
    echo "::warning::$what exited at launch; see smoke-$what.log"
  fi
}

native() { if command -v cygpath >/dev/null; then cygpath -w "$1"; else printf '%s' "$1"; fi; }
alive() {
  if [ "$os" = windows ]; then tasklist | grep -qi '^mareader.exe'
  else kill -0 "$1" 2>/dev/null; fi
}
stop() {
  if [ "$os" = windows ]; then taskkill //F //IM mareader.exe >/dev/null 2>&1 || true
  else kill "$1" 2>/dev/null || true; fi
  sleep 2
}
shot() { # best effort: proof the window drew
  case $os in
    linux) import -window root "$report/$1.png" 2>/dev/null || true ;;
    macos) screencapture -x "$report/$1.png" 2>/dev/null || true ;;
  esac
}

linux() {
  local deb appimage pacman_tar want_arch want_deb want_elf
  deb=$(one "$out"/*.deb) || { bad "one .deb"; return; }
  appimage=$(one "$out"/*.AppImage) || { bad "one AppImage"; return; }
  pacman_tar=$(one "$out"/*.tar.gz) || { bad "one pacman .tar.gz"; return; }
  case ${CARGO_BUILD_TARGET:-$(uname -m)} in
    aarch64* | arm64*) want_arch=aarch64; want_deb=arm64; want_elf="ARM aarch64" ;;
    *) want_arch=x86_64; want_deb=amd64; want_elf="x86-64" ;;
  esac
  check "the pacman archive holds usr/bin/mareader" \
    bash -c "tar -tzf '$pacman_tar' | grep -qE '^(\./)?usr/bin/mareader$'"
  check "PKGBUILD targets $want_arch" \
    grep -qF "arch=('$want_arch')" "$out/PKGBUILD"
  check "PKGBUILD names the archive" \
    grep -qF "source=(\"$(basename "$pacman_tar")\")" "$out/PKGBUILD"
  check "PKGBUILD names its libraries" grep -q "^depends=(fontconfig" "$out/PKGBUILD"
  check ".deb targets $want_deb" \
    bash -c "[ \"\$(dpkg-deb -f '$deb' Architecture)\" = '$want_deb' ]"
  check ".deb holds /usr/bin/mareader" \
    bash -c "dpkg-deb -c '$deb' | grep -qE ' (\./)?usr/bin/mareader$'"
  # The .deb paths may or may not start with ./, so match either.
  dpkg-deb --fsys-tarfile "$deb" |
    tar -xO --wildcards '*usr/share/applications/mareader.desktop' \
    >"$report/mareader.desktop" || bad ".deb has mareader.desktop"
  for mime in application/pdf application/epub+zip text/plain text/markdown; do
    check "desktop entry opens $mime" grep -q "^MimeType=.*$mime" "$report/mareader.desktop"
  done
  check "desktop entry passes files" grep -q '^Exec=mareader %F' "$report/mareader.desktop"
  check ".deb installs with its dependencies" sudo apt-get install -y "$(cd "$(dirname "$deb")" && pwd)/$(basename "$deb")"
  check "installed binary is $want_arch" \
    bash -c "file /usr/bin/mareader | grep -q '$want_elf'"
  check "every linked library resolves" bash -c '! ldd /usr/bin/mareader | grep -q "not found"'
  smoke deb /usr/bin/mareader
  check ".deb uninstalls" sudo apt-get remove -y mareader
  chmod +x "$appimage"
  # No FUSE on runners: the AppImage unpacks itself to run.
  APPIMAGE_EXTRACT_AND_RUN=1 smoke appimage "$appimage"
}

macos() {
  local app dmg bin plist want mnt sig
  app=$(one "$out"/*.app) || { bad "one .app"; return; }
  dmg=$(one "$out"/*.dmg) || { bad "one .dmg"; return; }
  bin="$app/Contents/MacOS/mareader"
  plist="$app/Contents/Info.plist"
  check "Info.plist is valid" plutil -lint "$plist"
  plutil -extract CFBundleDocumentTypes json -o "$report/types.json" "$plist" ||
    bad "Info.plist lists document types"
  for ext in pdf epub txt md; do
    check "the app opens .$ext" grep -q "\"$ext\"" "$report/types.json"
  done
  check "it runs on macOS 11 and later" \
    bash -c "[ \"\$(plutil -extract LSMinimumSystemVersion raw '$plist')\" = 11.0 ]"
  check "the bundle's signature verifies" codesign --verify --deep --strict "$app"
  case ${CARGO_BUILD_TARGET:-} in
    x86_64-apple-darwin) want=x86_64 ;;
    aarch64-apple-darwin) want=arm64 ;;
    *) want=$(uname -m) ;;
  esac
  check "the binary is $want" bash -c "lipo -archs '$bin' | grep -qw $want"
  check "it links only system libraries" \
    bash -c "! otool -L '$bin' | tail -n +2 | grep -vqE '^[[:space:]]*(/System/Library/|/usr/lib/)'"
  if [ "${HELPER:-}" = expected ]; then
    check "the Apple Intelligence helper is inside" test -x "$app/Contents/Resources/FMBridge"
  fi
  check "the .dmg's checksums hold" hdiutil verify "$dmg"
  # macOS refuses an ad hoc signed .dmg outright (tools/unsign-dmg.py).
  sig=$(codesign -dv "$app" 2>&1 || true)
  if [[ $sig == *Signature=adhoc* ]]; then
    check "an ad hoc build's .dmg is unsigned" bash -c "! codesign -d '$dmg'"
  fi
  mnt=$(mktemp -d)
  if hdiutil attach -nobrowse -readonly -mountpoint "$mnt" "$dmg" >>"$report/log.txt"; then
    check "the .dmg holds the app" test -d "$mnt/$(basename "$app")"
    check "the app in the .dmg is signed" \
      codesign --verify --deep --strict "$mnt/$(basename "$app")"
    check "the .dmg links to Applications" test -L "$mnt/Applications"
    hdiutil detach "$mnt" >>"$report/log.txt" || true
  else
    bad "the .dmg mounts"
  fi
  smoke app "$bin"
}

windows() {
  # Native tools read /S and friends as flags, not paths.
  export MSYS_NO_PATHCONV=1
  local formats setup msi home exe want_arch
  formats=",${FORMATS:-nsis,wix},"
  case ${CARGO_BUILD_TARGET:-$(uname -m)} in
    aarch64* | arm64*) want_arch=arm64 ;;
    *) want_arch=x64 ;;
  esac
  if [[ "$formats" == *,nsis,* ]]; then
    setup=$(one "$out"/*-setup.exe) || { bad "one -setup.exe"; return; }
    check "NSIS installs silently" "$setup" /S
    home="$(cygpath -u "$LOCALAPPDATA")/Mareader"
    check "it installs mareader.exe for this user" test -f "$home/mareader.exe"
    check "the installed mareader.exe is $want_arch" \
      pwsh -NoProfile -File tools/exe-arch.ps1 "$(cygpath -w "$home/mareader.exe")" "$want_arch"
    for ext in pdf epub txt md; do
      check ".$ext opens with Mareader" \
        bash -c "reg query 'HKCU\\Software\\Classes\\.$ext' /ve | grep -qi 'Document\\|Book'"
    done
    check "the installed mareader.exe carries its icon" \
      pwsh -NoProfile -File tools/exe-icons.ps1 "$(cygpath -w "$home/mareader.exe")"
    smoke nsis "$home/mareader.exe"
    check "NSIS uninstalls silently" "$home/uninstall.exe" /S
    # The uninstaller hands off to a copy of itself; wait for it.
    for _ in $(seq 30); do [ -f "$home/mareader.exe" ] || break; sleep 1; done
    check "the uninstall removes mareader.exe" test ! -f "$home/mareader.exe"
  fi
  if [[ "$formats" == *,wix,* ]]; then
    msi=$(one "$out"/*.msi) || { bad "one .msi"; return; }
    check "the MSI installs silently" msiexec /i "$(cygpath -w "$msi")" /qn "/l*v" "$(cygpath -w "$report/msi.log")"
    exe=$(find "/c/Program Files" -maxdepth 2 -iname mareader.exe | head -1)
    check "the MSI installs mareader.exe" test -n "$exe"
    if [ -n "$exe" ]; then
      check "the MSI mareader.exe is $want_arch" \
        pwsh -NoProfile -File tools/exe-arch.ps1 "$(cygpath -w "$exe")" "$want_arch"
      smoke msi "$exe"
    fi
    check "the MSI uninstalls silently" msiexec /x "$(cygpath -w "$msi")" /qn
  fi
}

case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=macos ;;
  MINGW* | MSYS* | CYGWIN*) os=windows ;;
  *) echo "unknown OS $(uname -s)" >&2; exit 2 ;;
esac
: >"$report/checks.txt"
say "== installers in $out ($os)"
manifest
$os
if [ "$failed" -eq 0 ]; then say "== every check passed"; else say "== some checks failed"; fi
exit "$failed"
