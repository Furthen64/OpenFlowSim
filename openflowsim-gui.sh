#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# VTK's QVTK widget currently needs XWayland for a stable native window.
export QT_QPA_PLATFORM="${QT_QPA_PLATFORM:-xcb}"
exec uv run python "$repo_root/tools/ofs_gui.py" "$@"
