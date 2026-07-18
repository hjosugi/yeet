#!/usr/bin/env bash
set -euo pipefail

binary="${1:-./target/release/yeet}"
binary="$(realpath "$binary")"
script="$(realpath "$0")"

if [[ "${YEET_ISOLATED_DBUS:-}" != "1" ]]; then
  exec env YEET_ISOLATED_DBUS=1 dbus-run-session -- "$script" "$binary"
fi

runtime="$(mktemp -d)"
chmod 700 "$runtime"
export XDG_RUNTIME_DIR="$runtime"
export WLR_BACKENDS=headless
export WLR_RENDERER=pixman
export WLR_LIBINPUT_NO_DEVICES=1

sway --config /dev/null --debug >"$runtime/sway.log" 2>&1 &
sway_pid=$!
yeet_pid=""
cleanup() {
  if [[ -n "$yeet_pid" ]]; then
    kill "$yeet_pid" 2>/dev/null || true
  fi
  kill "$sway_pid" 2>/dev/null || true
  rm -rf "$runtime"
}
trap cleanup EXIT

socket=""
ipc_socket=""
for _ in $(seq 1 100); do
  socket="$(find "$runtime" -maxdepth 1 -type s -name 'wayland-*' -printf '%f\n' | head -1)"
  ipc_socket="$(find "$runtime" -maxdepth 1 -type s -name 'sway-ipc.*.sock' | head -1)"
  if [[ -n "$socket" && -n "$ipc_socket" ]]; then
    break
  fi
  if ! kill -0 "$sway_pid" 2>/dev/null; then
    sed -n '1,200p' "$runtime/sway.log"
    exit 1
  fi
  sleep 0.1
done
if [[ -z "$socket" || -z "$ipc_socket" ]]; then
  sed -n '1,200p' "$runtime/sway.log"
  echo "Sway did not create a Wayland socket." >&2
  exit 1
fi

swaymsg --socket "$ipc_socket" create_output >/dev/null
swaymsg --socket "$ipc_socket" output HEADLESS-1 scale 1.5 >/dev/null
swaymsg --socket "$ipc_socket" output HEADLESS-2 scale 2 >/dev/null

set +e
env \
  XDG_CURRENT_DESKTOP=sway \
  WAYLAND_DISPLAY="$socket" \
  GDK_BACKEND=wayland \
  GSK_RENDERER=cairo \
  NO_AT_BRIDGE=1 \
  WAYLAND_DEBUG=client \
  timeout 5s "$binary" --hidden >"$runtime/yeet.log" 2>&1 &
yeet_pid=$!
set -e

initial_edge_count=0
for _ in $(seq 1 100); do
  initial_edge_count="$(
    grep -Ec 'zwlr_layer_shell_v1[^[:space:]]*\.get_layer_surface.*"yeet-edge-strip"' \
      "$runtime/yeet.log" || true
  )"
  if [[ "$initial_edge_count" -ge 2 ]]; then
    break
  fi
  if ! kill -0 "$yeet_pid" 2>/dev/null; then
    break
  fi
  sleep 0.05
done

if [[ "$initial_edge_count" -lt 2 ]]; then
  sed -n '1,240p' "$runtime/yeet.log"
  echo "Yeet did not create one edge layer surface for each headless output." >&2
  exit 1
fi

swaymsg --socket "$ipc_socket" output HEADLESS-1 scale 1.25 >/dev/null
swaymsg --socket "$ipc_socket" output HEADLESS-2 scale 1.75 >/dev/null
rebuilt_edge_count="$initial_edge_count"
for _ in $(seq 1 100); do
  rebuilt_edge_count="$(
    grep -Ec 'zwlr_layer_shell_v1[^[:space:]]*\.get_layer_surface.*"yeet-edge-strip"' \
      "$runtime/yeet.log" || true
  )"
  if [[ "$rebuilt_edge_count" -gt "$initial_edge_count" ]]; then
    break
  fi
  if ! kill -0 "$yeet_pid" 2>/dev/null; then
    break
  fi
  sleep 0.05
done

set +e
wait "$yeet_pid"
status=$?
set -e

if [[ "$status" -ne 124 ]]; then
  sed -n '1,240p' "$runtime/yeet.log"
  sed -n '1,200p' "$runtime/sway.log"
  echo "Yeet exited unexpectedly in the Sway layer-shell smoke test (status $status)." >&2
  exit 1
fi
if [[ "$rebuilt_edge_count" -le "$initial_edge_count" ]]; then
  sed -n '1,240p' "$runtime/yeet.log"
  echo "Yeet did not rebuild edge surfaces after fractional-scale changes." >&2
  exit 1
fi
if ! grep -Eq 'zwlr_layer_shell_v1[^[:space:]]*\.get_layer_surface.*"yeet-shelf"' "$runtime/yeet.log" ||
  ! grep -Eq 'zwlr_layer_shell_v1[^[:space:]]*\.get_layer_surface.*"yeet-edge-strip"' "$runtime/yeet.log"; then
  sed -n '1,240p' "$runtime/yeet.log"
  echo "Yeet stayed alive but did not request a layer-shell surface." >&2
  exit 1
fi

echo "Layer-shell shelf and per-output edges survived fractional-scale changes under headless Sway."
