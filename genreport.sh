#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
output_path="${1:-$repo_root/validation/reports/report.html}"
python="$repo_root/.venv/bin/python"

if [[ ! -x "$python" ]]; then
  printf 'Python virtual environment not found at %s. Create it with `uv sync`.\n' "$python" >&2
  exit 1
fi

"$python" "$repo_root/validation/render_report.py" \
  --reports-dir "$repo_root/validation/reports" \
  --output "$output_path"

printf 'HTML report written to %s\n' "$output_path"
