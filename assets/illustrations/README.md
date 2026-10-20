[Documentation](../../docs/README.md)

# Illustration sources

This directory contains the original branding and About-dialog artwork.
Follow [Generate application assets](../../docs/development/assets.md) for
requirements, commands, generated outputs, and preview instructions.

## Inventory

| Source | Purpose | Generated format |
| --- | --- | --- |
| [bunny/](bunny/README.md) | Application icon, title-bar icon, and About animation | Straight RGBA and platform icon files |
| `macaw-quill.svg` | About-dialog quill | 400 × 440 straight RGBA |
| `readme/banner.svg`, `readme/banner-compact.svg` | Repository README banners | Used directly as SVG |

The quill renderer supports paths, flat fills and strokes, and a path clip.
The bunny renderer uses resvg to preserve its gradients and transforms.
Generated rasters and review outputs are ignored by Git.

## Artwork rights and references

Original artwork and derived rasters are **all rights reserved** and excluded
from the code's BSD license. See [LICENSE](LICENSE).

Quill anatomical references:

- [Blue-and-yellow macaw feather](https://commons.wikimedia.org/wiki/File:Feather_of_a_Ara_ararauna_1.jpg)
- [Scarlet plumage](https://commons.wikimedia.org/wiki/File:Red_feathers1a_(8306378464).jpg)
