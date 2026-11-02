#!/usr/bin/env bash
set -u

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
os_id="unknown"
if [[ -r /etc/os-release ]]; then
  os_id="$(. /etc/os-release && printf '%s' "${ID:-unknown}")"
fi

all_ok=1
need_rust=0
need_uv=0
need_python=0
need_venv=0

printf 'Checking OpenFlowSim development requirements...\n\n'

for tool in rustc cargo; do
  if command -v "$tool" >/dev/null 2>&1; then
    printf 'OK   %s: %s\n' "$tool" "$("$tool" --version)"
  else
    printf 'MISS %s is not installed or not on PATH.\n' "$tool"
    need_rust=1
    all_ok=0
  fi
done

if command -v python3 >/dev/null 2>&1; then
  system_python_version="$(python3 -c 'import platform; print(platform.python_version())')"
  if python3 -c 'import sys; raise SystemExit(sys.version_info < (3, 14))'; then
    printf 'OK   system Python: %s\n' "$system_python_version"
  else
    printf 'INFO system Python is %s; the project requires Python 3.14 or newer.\n' "$system_python_version"
  fi
else
  printf 'INFO system python3 is not installed.\n'
fi

if command -v uv >/dev/null 2>&1; then
  printf 'OK   uv: %s\n' "$(uv --version)"
else
  printf 'MISS uv is not installed or not on PATH.\n'
  need_uv=1
  all_ok=0
fi

venv="$repo_root/.venv"
venv_python="$venv/bin/python"
venv_cfg="$venv/pyvenv.cfg"
if [[ -x "$venv_python" && -f "$venv_cfg" ]] && grep -Eq '^uv[[:space:]]*=' "$venv_cfg"; then
  venv_python_version="$("$venv_python" -c 'import platform; print(platform.python_version())')"
  if "$venv_python" -c 'import sys; raise SystemExit(sys.version_info < (3, 14))'; then
    printf 'OK   .venv was created by uv with Python %s.\n' "$venv_python_version"
  else
    printf 'MISS .venv uses Python %s; the project requires Python 3.14 or newer.\n' "$venv_python_version"
    need_python=1
    need_venv=1
    all_ok=0
  fi

  if command -v uv >/dev/null 2>&1; then
    if (cd "$repo_root" && uv sync --locked --check); then
      printf 'OK   .venv is synchronized with uv.lock.\n'
    else
      printf 'MISS .venv is not synchronized with the project lockfile.\n'
      need_venv=1
      all_ok=0
    fi
  fi
elif [[ -e "$venv" ]]; then
  printf 'MISS .venv exists but was not created by uv, or is incomplete.\n'
  need_venv=1
  all_ok=0
else
  printf 'MISS .venv has not been initialized with uv.\n'
  need_python=1
  need_venv=1
  all_ok=0
fi

if (( all_ok )); then
  printf '\nAll required toolchains and the uv environment are ready.\n'
  exit 0
fi

printf '\nSetup help:\n'
if [[ "$os_id" != "ubuntu" ]]; then
  printf 'Ubuntu setup commands are provided for Ubuntu only (detected: %s).\n' "$os_id"
  printf 'Install the missing tools for your operating system, then rerun ./checkreqs.sh.\n'
  exit 1
fi

if (( need_rust )); then
  cat <<'EOF'
Install the Rust toolchain on Ubuntu:
  sudo apt update
  sudo apt install -y build-essential curl ca-certificates
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  source "$HOME/.cargo/env"

EOF
fi

if (( need_uv )); then
  cat <<'EOF'
Install uv:
  curl -LsSf https://astral.sh/uv/install.sh | sh
  source "$HOME/.local/bin/env"

EOF
fi

if (( need_python )); then
  cat <<'EOF'
Install the required Python version with uv:
  uv python install 3.14

EOF
fi

if (( need_venv )); then
  printf 'Initialize or synchronize the project environment:\n  cd %q\n  uv sync --locked\n\n' "$repo_root"
  if [[ -e "$venv" ]]; then
    printf 'If .venv was created outside uv, preserve anything needed before replacing it.\n\n'
  fi
fi

printf 'Rerun ./checkreqs.sh after setup.\n'
exit 1
