#!/usr/bin/env bash
set -euo pipefail

# GitHub's Linux runners have no physical GPU. Run Vulkan rendering tests
# through Mesa Lavapipe.
: "${GITHUB_ENV:?This script configures a GitHub Actions job}"
: "${RUNNER_TEMP:?GitHub Actions runner temporary directory is required}"

shopt -s nullglob
lavapipe_manifests=(/usr/share/vulkan/icd.d/lvp_icd*.json)
if (( ${#lavapipe_manifests[@]} != 1 )); then
    echo "Expected one Mesa Lavapipe ICD; install mesa-vulkan-drivers." >&2
    exit 1
fi

export VK_DRIVER_FILES="${lavapipe_manifests[0]}"
export VK_ICD_FILENAMES="$VK_DRIVER_FILES"
export WGPU_BACKEND=vulkan
export XDG_RUNTIME_DIR
XDG_RUNTIME_DIR="$(mktemp -d "$RUNNER_TEMP/fragile-vulkan.XXXXXX")"
chmod 700 "$XDG_RUNTIME_DIR"

# Fail before compiling if the software Vulkan device cannot be enumerated.
xvfb-run -a vulkaninfo --summary

{
    echo "VK_DRIVER_FILES=$VK_DRIVER_FILES"
    echo "VK_ICD_FILENAMES=$VK_ICD_FILENAMES"
    echo "WGPU_BACKEND=$WGPU_BACKEND"
    echo "XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR"
} >> "$GITHUB_ENV"
