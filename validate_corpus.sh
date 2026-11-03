#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
run_dir="$repo_root/validation/runs/$(date -u +%Y%m%dT%H%M%SZ)-$$"
make_html=1

usage() {
  cat <<EOF
Usage: $0 [--run-dir DIR] [--no-html]

Runs supported corpus cases, compares solver output with references, performs
available analytical checks, and writes an aggregate JSON/Markdown report.
EOF
}

while (($#)); do
  case "$1" in
    --run-dir)
      run_dir="$2"
      [[ "$run_dir" = /* ]] || run_dir="$repo_root/$run_dir"
      shift 2
      ;;
    --no-html)
      make_html=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'Unknown option: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -e "$run_dir" ]]; then
  printf 'Run directory already exists: %s\nChoose a new path with --run-dir.\n' "$run_dir" >&2
  exit 2
fi
mkdir -p "$run_dir"/{results,reports,logs,status}
cd "$repo_root"

printf 'Starting corpus validation in %s\n' "$run_dir"

for case_dir in "$repo_root"/corpus/*; do
  [[ -d "$case_dir" && -f "$case_dir/case.toml" ]] || continue
  case_name="$(basename "$case_dir")"
  status_file="$run_dir/status/$case_name.txt"

  skip_reason=""
  if ! grep -Eq '^[[:space:]]*solver[[:space:]]*=[[:space:]]*"potentialFoam"' "$case_dir/case.toml"; then
    skip_reason="case metadata does not declare potentialFoam"
  elif [[ ! -f "$case_dir/0/U" && ! -f "$case_dir/0/U.orig" ]]; then
    skip_reason="no initial velocity field at 0/U or 0/U.orig"
  elif [[ ! -d "$case_dir/constant/polyMesh" ]] && ! grep -q 'cells_per_axis' "$case_dir/case.toml"; then
    skip_reason="no generated polyMesh or structured cell-count metadata"
  elif [[ ! -f "$repo_root/reference/$case_name/U" || ! -f "$repo_root/reference/$case_name/phi" ]]; then
    skip_reason="matching reference U/phi fields are unavailable"
  fi

  if [[ -n "$skip_reason" ]]; then
    printf 'SKIP\n%s\n' "$skip_reason" > "$status_file"
    printf 'SKIP %s: %s\n' "$case_name" "$skip_reason"
    continue
  fi

  printf 'RUN  %s\n' "$case_name"
  solver_log="$run_dir/logs/$case_name.solver.log"
  solver_output="$run_dir/results/$case_name"
  if cargo run --quiet -p foamcore-solver -- run "$case_dir" --output "$solver_output" > "$solver_log" 2>&1; then
    printf '0\n' > "$run_dir/status/$case_name.solver.exit"
  else
    solver_exit=$?
    printf '%s\n' "$solver_exit" > "$run_dir/status/$case_name.solver.exit"
    printf 'ERROR %s: solver exited %s (see %s)\n' "$case_name" "$solver_exit" "$solver_log"
    continue
  fi

  compare_log="$run_dir/logs/$case_name.compare.log"
  if cargo run --quiet -p foamcore-validation -- compare "$case_name" \
    --results-dir "$run_dir/results" \
    --ref-dir "$repo_root/reference" \
    --tolerances "$repo_root/validation/tolerances.toml" \
    --report "$run_dir/reports/$case_name.comparison.json" > "$compare_log" 2>&1; then
    printf '0\n' > "$run_dir/status/$case_name.compare.exit"
  else
    compare_exit=$?
    printf '%s\n' "$compare_exit" > "$run_dir/status/$case_name.compare.exit"
    printf 'COMPARE %s: exited %s (see %s)\n' "$case_name" "$compare_exit" "$compare_log"
  fi

  if grep -Eq '^[[:space:]]*type[[:space:]]*=[[:space:]]*"analytical"' "$case_dir/case.toml"; then
    analytical_log="$run_dir/logs/$case_name.analytical.log"
    if cargo run --quiet -p foamcore-validation -- analytical "$case_name" \
      --results-dir "$run_dir/results" \
      --report "$run_dir/reports/$case_name.analytical.json" > "$analytical_log" 2>&1; then
      printf '0\n' > "$run_dir/status/$case_name.analytical.exit"
    else
      analytical_exit=$?
      printf '%s\n' "$analytical_exit" > "$run_dir/status/$case_name.analytical.exit"
      printf 'ANALYTICAL %s: exited %s (see %s)\n' "$case_name" "$analytical_exit" "$analytical_log"
    fi
  fi
done

python="$repo_root/.venv/bin/python"
if [[ ! -x "$python" ]]; then
  python="$(command -v python3 || true)"
fi
if [[ -z "$python" ]]; then
  printf 'Python 3 is required to aggregate the run reports.\n' >&2
  exit 2
fi

if "$python" "$repo_root/validation/aggregate_corpus_run.py" \
  --run-dir "$run_dir" --corpus-dir "$repo_root/corpus" \
  --reference-dir "$repo_root/reference"; then
  validation_exit=0
else
  validation_exit=$?
fi

if (( make_html )) && [[ -x "$repo_root/.venv/bin/python" ]]; then
  "$repo_root/genreport.sh" --reports-dir "$run_dir/reports" --output "$run_dir/report.html" \
    || printf 'HTML rendering failed; JSON and Markdown reports remain available.\n' >&2
elif (( make_html )); then
  printf 'HTML rendering skipped because .venv/bin/python is unavailable.\n' >&2
fi

printf 'Run artifacts: %s\n' "$run_dir"
exit "$validation_exit"
