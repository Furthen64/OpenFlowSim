#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
corpus_dir="$repo_root/corpus"
reference_dir="$repo_root/reference"
python="$repo_root/.venv/bin/python"
if [[ ! -x "$python" ]]; then
  python="$(command -v python3 || true)"
fi
if [[ -z "$python" ]]; then
  printf 'Python 3 is required to read case.toml metadata.\n' >&2
  exit 2
fi

while (($#)); do
  case "$1" in
    --corpus-dir)
      corpus_dir="$2"
      shift 2
      ;;
    --reference-dir)
      reference_dir="$2"
      shift 2
      ;;
    -h|--help)
      printf 'Usage: %s [--corpus-dir DIR] [--reference-dir DIR]\n' "$0"
      exit 0
      ;;
    *)
      printf 'Unknown option: %s\n' "$1" >&2
      exit 2
      ;;
  esac
done

exec "$python" "$repo_root/validation/corpus_overview.py" \
  --corpus-dir "$corpus_dir" --reference-dir "$reference_dir"
