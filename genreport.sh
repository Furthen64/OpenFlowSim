#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
reports_dir="$repo_root/validation/reports"
output_path=""

while (($#)); do
  case "$1" in
    --reports-dir)
      reports_dir="$(cd "$2" && pwd)"
      shift 2
      ;;
    --output)
      output_path="$2"
      shift 2
      ;;
    -h|--help)
      printf 'Usage: %s [OUTPUT.html] [--reports-dir DIR] [--output FILE]\n' "$0"
      exit 0
      ;;
    *)
      output_path="$1"
      shift
      ;;
  esac
done
if [[ -z "$output_path" ]]; then
  output_path="$reports_dir/report.html"
fi
python="$repo_root/.venv/bin/python"

if [[ ! -x "$python" ]]; then
  printf 'Python virtual environment not found at %s. Create it with `uv sync`.\n' "$python" >&2
  exit 1
fi

"$python" "$repo_root/validation/render_report.py" \
  --reports-dir "$reports_dir" \
  --output "$output_path"

printf 'HTML report written to %s\n' "$output_path"
