#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != Darwin || $# != 1 ]]; then
    echo "Usage (macOS): bash scripts/package-macos.sh NEW_PACKAGE_DIRECTORY" >&2
    exit 1
fi
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
loader="$(brew --prefix vulkan-loader)"
molten="$(brew --prefix molten-vk)"
# Refuse an existing destination; never mix stale runtime libraries into a build.
mkdir "$1"
package="$(cd "$1" && pwd)"
mkdir -p "$package"/{lib,libexec,share/vulkan/icd.d,licenses}
cp "$repo_root/target/release/fragile-notepad" "$package/libexec/"
cp "$repo_root/scripts/macos-launcher.sh" "$package/fragile-notepad"
chmod +x "$package/fragile-notepad"
cp -L "$loader/lib/libvulkan.1.dylib" "$package/lib/"
ln -s libvulkan.1.dylib "$package/lib/libvulkan.dylib"
cp -L "$molten/lib/libMoltenVK.dylib" "$package/lib/"
cp "$repo_root/LICENSE" "$package/"
cp "$repo_root/assets/icons/NOTICE.txt" "$package/ICON-NOTICES.txt"
# Cargo writes the notice for this exact artifact, including the complete OFL.
# A fresh local build may not have the explicit CI preparation directory.
font_notices="$repo_root/target/release/FONT-NOTICES.txt"
if [[ -s "$font_notices" ]]; then
    cp "$font_notices" "$package/"
    if [[ -d "$repo_root/target/font-profiles/prepared/licenses" ]]; then
        cp -R "$repo_root/target/font-profiles/prepared/licenses" "$package/font-licenses"
    fi
fi

python3 - "$molten/etc/vulkan/icd.d/MoltenVK_icd.json" "$package" <<'PY'
import json
from pathlib import Path
import sys
manifest = json.loads(Path(sys.argv[1]).read_text())
manifest["ICD"]["library_path"] = "../../../lib/libMoltenVK.dylib"
destination = Path(sys.argv[2]) / "share/vulkan/icd.d/MoltenVK_icd.json"
destination.write_text(json.dumps(manifest, indent=2) + "\n")
PY

# MoltenVK statically includes libraries such as SPIRV-Cross and cereal. Include
# their notices too, using the installed formula's exact source revisions.
python3 "$repo_root/scripts/collect-vulkan-licenses.py" \
    --loader-formula "$loader/.brew/vulkan-loader.rb" \
    --molten-formula "$molten/.brew/molten-vk.rb" \
    --output "$package/licenses"
brew info --json=v2 vulkan-loader molten-vk > "$package/licenses/vulkan-build.json"

# Make every bundled library relocatable; reject unresolved Homebrew paths.
for library in "$package/lib/libvulkan.1.dylib" "$package/lib/libMoltenVK.dylib"; do
    install_name_tool -id "@rpath/$(basename "$library")" "$library"
    while IFS= read -r dependency; do
        case "$dependency" in
            /System/Library/*|/usr/lib/*) continue ;;
        esac
        name="$(basename "$dependency")"
        if [[ ! -f "$package/lib/$name" ]]; then
            echo "Unbundled dependency: $library -> $dependency" >&2
            exit 1
        fi
        if [[ "$name" != "$(basename "$library")" ]]; then
            install_name_tool -change "$dependency" "@loader_path/$name" "$library"
        fi
    done < <(otool -L "$library" | tail -n +2 | awk '{print $1}')
    codesign --force --sign - --timestamp=none "$library"
    codesign --verify --strict "$library"
done
(
    cd "$package"
    shasum -a 256 lib/libvulkan.1.dylib lib/libMoltenVK.dylib libexec/fragile-notepad \
        > licenses/vulkan-sha256.txt
)
"$package/fragile-notepad" --version
