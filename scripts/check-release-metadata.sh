#!/usr/bin/env bash
set -euo pipefail

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
