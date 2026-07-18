#!/usr/bin/env bash
set -euo pipefail

mode="${1:-}"
if [[ -n "$mode" && "$mode" != "--tagged" ]]; then
  printf 'Usage: %s [--tagged]\n' "$0" >&2
  exit 2
fi

fail() {
  printf 'release metadata: %s\n' "$*" >&2
  exit 1
}

version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)"
[[ -n "$version" ]] || fail "Cargo package version is missing"

assert_contains() {
  local path="$1"
  local expected="$2"
  grep -Fqx "$expected" "$path" ||
    fail "$path does not contain the expected line: $expected"
}

root_lock_version="$(
  awk '
    $0 == "name = \"yeet\"" { in_package = 1; next }
    in_package && /^version = / {
      gsub(/^version = "|".*$/, "")
      print
      exit
    }
  ' Cargo.lock
)"
[[ "$root_lock_version" == "$version" ]] ||
  fail "Cargo.lock has Yeet $root_lock_version, expected $version"

assert_contains packaging/nix/default.nix "  version = \"$version\";"
assert_contains packaging/windows/yeet.iss "  #define MyAppVersion \"$version\""
assert_contains packaging/linux/yeet.1 \
  ".TH YEET 1 \"July 2026\" \"Yeet $version\" \"User Commands\""
assert_contains README.md "version=$version"
assert_contains README.ja.md "version=$version"
assert_contains docs/windows-release.md "\$version = \"$version\""

appstream_version="$(
  sed -n 's/.*<release version="\([^"]*\)".*/\1/p' \
    packaging/linux/io.github.hjosugi.Yeet.metainfo.xml |
    head -1
)"
[[ "$appstream_version" == "$version" ]] ||
  fail "AppStream newest release is $appstream_version, expected $version"

grep -Fq "targets Yeet $version." docs/release-checklist.md ||
  fail "release checklist does not target $version"
grep -Fq "tagging v$version" docs/release-checklist.md ||
  fail "release checklist does not name v$version"

if rg -n 'hjosugi/wayland-yeet|name = "wayland-yeet"' \
  --glob '!target/**' --glob '!scripts/check-release-metadata.sh' .; then
  fail "legacy repository or Cargo package name remains"
fi

printf 'Release-independent metadata consistently targets Yeet %s.\n' "$version"

if [[ "$mode" == "--tagged" ]]; then
  tag="v$version"
  commit="$(git rev-parse "$tag^{}")" ||
    fail "tag $tag does not exist"
  git_pkgver="$(
    git describe --long --tags --abbrev=7 "$tag^{}" |
      sed 's/^v//;s/\([^-]*-g\)/r\1/;s/-/./g'
  )"

  assert_contains packaging/arch/PKGBUILD "pkgver=$version"
  assert_contains packaging/arch/PKGBUILD-git "pkgver=$git_pkgver"
  grep -Fq "tag: $tag" packaging/flatpak/io.github.hjosugi.Yeet.yml ||
    fail "Flatpak manifest does not target $tag"
  grep -Fq "commit: $commit" packaging/flatpak/io.github.hjosugi.Yeet.yml ||
    fail "Flatpak manifest does not pin $commit"

  archive="$(mktemp)"
  trap 'rm -f "$archive"' EXIT
  curl -fsSL \
    "https://github.com/hjosugi/yeet/archive/refs/tags/$tag.tar.gz" \
    -o "$archive"
  archive_hash="$(sha256sum "$archive" | cut -d' ' -f1)"
  grep -Fq "sha256sums=('$archive_hash')" packaging/arch/PKGBUILD ||
    fail "Arch archive hash does not match the published $tag source archive"

  windows_sums="$(
    curl -fsSL \
      "https://github.com/hjosugi/yeet/releases/download/$tag/SHA256SUMS-windows.txt"
  )"
  portable_hash="$(
    sed -n "s/ .*yeet-$version-windows-x64\\.zip\$//p" <<<"$windows_sums"
  )"
  [[ -n "$portable_hash" ]] ||
    fail "portable ZIP is missing from the published Windows checksums"
  python - "$version" "$portable_hash" <<'PY'
import json
import sys

version, expected_hash = sys.argv[1:]
with open("bucket/yeet.json", encoding="utf-8") as manifest_file:
    manifest = json.load(manifest_file)
portable = manifest["architecture"]["64bit"]
expected_url = (
    f"https://github.com/hjosugi/yeet/releases/download/v{version}/"
    f"yeet-{version}-windows-x64.zip"
)
expected_extract_dir = f"yeet-{version}-windows-x64"
if manifest["version"] != version:
    raise SystemExit("Scoop manifest version does not match the release")
if portable["url"] != expected_url:
    raise SystemExit("Scoop manifest URL does not match the release")
if portable["extract_dir"] != expected_extract_dir:
    raise SystemExit("Scoop extract_dir does not match the release")
if portable["hash"] != expected_hash:
    raise SystemExit("Scoop hash does not match the published portable ZIP")
PY

  if rg -n '^- \[ \]' docs/release-checklist.md; then
    fail "release checklist still has incomplete items"
  fi
  printf 'Tagged metadata matches published %s artifacts.\n' "$tag"
fi
