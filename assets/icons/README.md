[Documentation](../../docs/README.md)

# Icon sources

See [Compile UI icons](../../docs/development/assets.md#compile-ui-icons) for
build instructions.

## Inventory

| Directory | Contents | Format |
| --- | --- | --- |
| `colored/svg/` | Original application action icons | SVG on a 22-unit canvas |
| `heroicons/svg/` | Modified monochrome control icons | SVG using `currentColor` |
| `bootstrap/svg/` | Modified shortcut-key and pin icons | SVG using `currentColor` |
| `file-types/svg/` | Large and small file-type artwork | SVG; entries and extensions in [catalog.json](file-types/catalog.json) |

Cargo compiles `colored`, `heroicons`, and `bootstrap` into Rust vector geometry
in its `OUT_DIR`. The application draws these paths at the requested size in both
rendering backends. The `file-types` collection is maintained as SVG sources and
is not embedded in the application.

The `other.svg` and `other-small.svg` sources provide generic file icons. Their
`other` catalog entry uses an empty extension list for files without a listed
extension.

## SVG contract

The UI icon compiler preserves straight lines, quadratic and cubic curves, and
elliptical arcs. It supports flat six-digit hexadecimal colors, `currentColor`,
inherited paint on SVG groups, fill rules, and stroke widths, caps, and joins.
Use `fill-rule="evenodd"` for filled shapes with holes. Transforms, gradients,
CSS styles, images, and text are rejected during compilation. The file-type SVGs
use additional SVG features and are not processed by this compiler.

## Licenses and distribution

- [Colored artwork](colored/LICENSE) is original project work, **all rights
  reserved**, and excluded from the code's BSD license.
- Modified [Heroicons](heroicons/LICENSE) and [Bootstrap Icons](bootstrap/LICENSE)
  retain their MIT notices.
- [File-type artwork](file-types/LICENSE) has its own MIT grant and third-party
  notices. The grant does not license the standalone logo or other project
  artwork.

Redistribute [NOTICE.txt](NOTICE.txt) as `ICON-NOTICES.txt`. Preserve the
file-type collection's [license and third-party notices](file-types/LICENSE)
when distributing that artwork.
