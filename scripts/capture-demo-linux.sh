#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/capture-demo-linux.sh MODE [OUTPUT_DIR] [--force]

MODE is one of:
  loop   Record yeet-linux-demo.webm and convert it to GIF.
  light  Capture yeet-linux-light.png.
  dark   Capture yeet-linux-dark.png.

Set YEET_CAPTURE_GEOMETRY to a wf-recorder/grim geometry such as
"1200,180 620x720" to avoid the interactive slurp selection.
EOF
}

mode="${1:-}"
output_dir="${2:-docs/screenshots}"
force="${3:-}"

case "$mode" in
  loop | light | dark) ;;
  -h | --help)
    usage
    exit 0
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac

if [[ -n "$force" && "$force" != "--force" ]]; then
  usage >&2
  exit 2
fi

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'error: required command not found: %s\n' "$1" >&2
    exit 1
  fi
}

refuse_existing() {
  local path
  for path in "$@"; do
    if [[ -e "$path" && "$force" != "--force" ]]; then
      printf 'error: refusing to overwrite %s (pass --force after OUTPUT_DIR)\n' "$path" >&2
      exit 1
    fi
  done
}

select_geometry() {
  if [[ -n "${YEET_CAPTURE_GEOMETRY:-}" ]]; then
    printf '%s\n' "$YEET_CAPTURE_GEOMETRY"
    return
  fi
  require_command slurp
  local selected
  selected="$(slurp)"
  if [[ -z "$selected" ]]; then
    printf 'error: no capture region selected\n' >&2
    exit 1
  fi
  printf '%s\n' "$selected"
}

countdown() {
  local label="$1"
  local second
  printf '%s starts in ' "$label"
  for second in 3 2 1; do
    printf '%s ' "$second"
    sleep 1
  done
  printf '\n'
}

mkdir -p "$output_dir"
geometry="$(select_geometry)"

if [[ "$mode" == "light" || "$mode" == "dark" ]]; then
  require_command grim
  output="$output_dir/yeet-linux-$mode.png"
  refuse_existing "$output"
  printf 'Confirm that Yeet is using the %s theme and contains only demo data.\n' "$mode"
  countdown "Screenshot"
  grim -g "$geometry" "$output"
  printf 'Wrote %s\n' "$output"
  exit 0
fi

require_command wf-recorder
require_command ffmpeg
encoders="$(ffmpeg -hide_banner -encoders 2>/dev/null)"
if [[ "$encoders" != *libvpx-vp9* ]]; then
  printf 'error: ffmpeg does not provide the libvpx-vp9 encoder\n' >&2
  exit 1
fi

webm="$output_dir/yeet-linux-demo.webm"
gif="$output_dir/yeet-linux-demo.gif"
refuse_existing "$webm" "$gif"
raw="$(mktemp --suffix=.yeet-demo.mkv)"
trap 'rm -f "$raw"' EXIT

cat <<'EOF'
Record the full loop documented in docs/demo-capture.md. Press Ctrl+C after
the shelf hides again. Synthetic drag input is intentionally not generated.
EOF
countdown "Recording"
set +e
wf-recorder -g "$geometry" -f "$raw"
status=$?
set -e
if [[ "$status" -ne 0 && "$status" -ne 130 ]]; then
  printf 'error: wf-recorder exited with status %s\n' "$status" >&2
  exit "$status"
fi
if [[ ! -s "$raw" ]]; then
  printf 'error: wf-recorder did not produce a recording\n' >&2
  exit 1
fi

overwrite=()
if [[ "$force" == "--force" ]]; then
  overwrite=(-y)
else
  overwrite=(-n)
fi
ffmpeg -hide_banner -loglevel warning "${overwrite[@]}" -i "$raw" \
  -an -c:v libvpx-vp9 -crf 32 -b:v 0 -pix_fmt yuv420p "$webm"
ffmpeg -hide_banner -loglevel warning "${overwrite[@]}" -i "$webm" \
  -filter_complex '[0:v]fps=12,split[s0][s1];[s0]palettegen=max_colors=128[p];[s1][p]paletteuse=dither=bayer' \
  -loop 0 "$gif"
printf 'Wrote %s and %s\n' "$webm" "$gif"
