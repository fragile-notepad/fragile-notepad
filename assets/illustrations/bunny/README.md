[Documentation](../../../docs/README.md) / [Illustrations](../README.md)

# Bunny artwork

`app.svg` is the supplied BunnyNotebook master. `title-bar.svg` uses the same
character without its background. The master also supplies the About dialog's
background, paper, bunny, and blink frames.

## Source contract

[generate_app_icons.py](../../../scripts/generate_app_icons.py) renders the
sources with resvg and Pillow. Preserve the named `face` group and its two eye
groups, and the `loose_paper` group and its two paper groups. The generator uses
those groups to derive blink frames and independent About layers.

## Generated assets

| Output | Size or contents |
| --- | --- |
| `app.rgba`, `app-half.rgba`, `app-closed.rgba` | 256 × 256 application and blink frames |
| `about-background.rgba`, `about-paper.rgba`, `about-bunny.rgba` | 384 × 384 About layers |
| `about-bunny-half.rgba`, `about-bunny-closed.rgba` | 384 × 384 blink frames |
| `title-bar.rgba` | 64 × 64 transparent title-bar icon |
| `target/app-icons/app.ico` | Windows icon with multiple sizes |
| `target/app-icons/app.icns`, `target/app-icons/app.png` | macOS icon and 1,024 × 1,024 PNG |

All `.rgba` files contain straight RGBA pixels. Generated files are ignored by
Git. See [Generate application assets](../../../docs/development/assets.md)
for regeneration and About/title-bar animation previews, including Vulkan.

## Artwork rights

Sources and derivatives are **all rights reserved** under the
[illustration artwork license](../LICENSE) and excluded from the code's BSD
license.
