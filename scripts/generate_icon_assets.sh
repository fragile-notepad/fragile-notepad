#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

python3 scripts/rasterize_illustrations.py
python3 scripts/generate_app_icons.py
python3 scripts/generate_file_type_icons.py
