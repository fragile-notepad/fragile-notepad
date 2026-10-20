[Documentation](../../docs/README.md)

# Icon sources

This directory contains editable icon sources and their artwork notices.
Follow [Generate application assets](../../docs/development/assets.md) to
regenerate the application's embedded rasters.

## Inventory

| Directory | Contents | Format |
| --- | --- | --- |
| `colored/svg/` | Original application action icons | SVG on a 22-unit canvas |
| `heroicons/svg/` | Modified monochrome control icons | SVG using `currentColor` |
| `bootstrap/svg/` | Modified shortcut-key and pin icons | SVG using `currentColor` |
| `file-types/svg/` | Large and small file-type artwork | SVG; entries and extensions in [catalog.json](file-types/catalog.json) |

The standard generation scripts rasterize `colored`, `heroicons`, and
`bootstrap` into 22 × 22 straight RGBA files in each family's `rgba/` directory.
Generated rasters are ignored by Git. The `file-types` collection is maintained
as SVG sources and is not processed by those scripts.

## SVG contract

The small-icon rasterizer supports paths, flat six-digit hexadecimal colors,
`currentColor`, and rounded caps and joins. Keep these sources free of transforms,
inherited group paint, and gradients. Use `fill-rule="evenodd"` for filled shapes
with holes. The file-type SVGs use a broader SVG feature set and are outside this
rasterizer's contract.

## Licenses and distribution

- [Colored artwork](colored/LICENSE) is original project work, **all rights
  reserved**, and excluded from the code's BSD license.
- Modified [Heroicons](heroicons/LICENSE) and [Bootstrap Icons](bootstrap/LICENSE)
  retain their MIT notices.
- [File-type artwork](file-types/LICENSE) has its own MIT grant and third-party
  notices. The grant limits reproduction of its bunny watermark to those icons;
  it does not license the standalone logo or other project artwork.

Redistribute [NOTICE.txt](NOTICE.txt) as `ICON-NOTICES.txt`. Preserve the
file-type collection's [license and third-party notices](file-types/LICENSE)
when distributing that artwork.
