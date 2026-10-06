#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$repo_root"

cargo fmt --package fragile-notepad --check
python3 -m unittest discover -s scripts -p 'test_*font_profiles.py'
case "$(uname -s)" in
    Darwin) font_target_os=macos ;;
    Linux) font_target_os=linux ;;
    *) echo "Unsupported CI font target: $(uname -s)" >&2; exit 1 ;;
esac
python3 scripts/prepare_font_profiles.py --out-dir target/font-profiles/prepared \
    --target-os "$font_target_os" --cache-dir target/font-profiles
bash scripts/generate_icon_assets.sh
python3 scripts/test_icon_assets.py

# cargo test also compiles the application and examples.
if [[ "$(uname -s)" == "Linux" ]] && command -v xvfb-run >/dev/null 2>&1; then
    WINIT_UNIX_BACKEND=x11 xvfb-run -a cargo test
else
    cargo test
fi

cargo test --locked -p iced_wgpu --lib
cargo test --locked -p cryoglyph --lib

cargo check --no-default-features
