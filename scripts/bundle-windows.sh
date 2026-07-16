#!/usr/bin/env bash
# Assemble the self-contained portable Windows bundle from a release build.
#
# Run inside the MSYS2 UCRT64 environment after `cargo build --release`. Both the
# release workflow (to ship the ZIP and feed the Inno Setup installer) and CI (to
# smoke-test the bundle) call this, so the two can never drift. Prints the bundle
# directory name on success.
set -euo pipefail

version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)"
root="yeet-${version}-windows-x64"

rm -rf "$root"
mkdir -p "$root/lib" "$root/share/glib-2.0"
cp target/release/yeet.exe "$root/"
# Copy the UCRT64 DLLs yeet.exe links against.
ldd target/release/yeet.exe | awk '/\/ucrt64\/bin\/.*\.dll/ { print $3 }' | xargs -r -I{} cp {} "$root/"
cp -r /ucrt64/lib/gdk-pixbuf-2.0 "$root/lib/gdk-pixbuf-2.0"
cp -r /ucrt64/share/glib-2.0/schemas "$root/share/glib-2.0/"

# GtkApplication registers on a D-Bus session bus for single-instance argument
# forwarding; on Windows GLib autolaunches that bus by spawning gdbus.exe.
# Without it GIO logs "win32 session dbus binary not found" and the app fails to
# start. The gspawn helpers back GLib process spawning (GAppInfo opening
# files/URIs in the default app). ldd cannot discover these standalone
# executables, so copy them explicitly.
cp /ucrt64/bin/gdbus.exe "$root/"
cp /ucrt64/bin/gspawn-win64-helper.exe "$root/"
cp /ucrt64/bin/gspawn-win64-helper-console.exe "$root/"

# Fail before publishing if the bundle is not self-contained: every non-system
# DLL that any bundled executable needs must also live in the bundle.
missing=0
for exe in "$root"/*.exe; do
  while read -r dll; do
    base="$(basename "$dll")"
    if [ ! -f "$root/$base" ]; then
      echo "bundle-windows: $(basename "$exe") needs $base, which is not bundled" >&2
      missing=1
    fi
  done < <(ldd "$exe" | awk '/\/ucrt64\/bin\/.*\.dll/ { print $3 }')
done
if [ "$missing" -ne 0 ]; then
  echo "bundle-windows: portable bundle is incomplete" >&2
  exit 1
fi

echo "$root"
