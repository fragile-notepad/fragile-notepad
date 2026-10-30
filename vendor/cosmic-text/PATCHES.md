# Local changes to cosmic-text 0.19.0

- Base package version: `0.19.0`; no pinned base revision recorded
- Upstream: https://github.com/pop-os/cosmic-text
- License: [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)

The upstream package source, manifests, tests, fixtures, and license files are
preserved here. The application supplies fonts from the operating system; this
patch does not contain or generate Apple font data.

## Linux generic fonts

Set default family names before loading system fonts so Fontconfig's generic
family choices survive initialization. Iced selects an installed Linux font
when a configured family is missing.

## Requested font weights

Keep the requested family's closest face when its exact weight is unavailable.
Previously, a medium or semibold request could skip the UI family and select an
unrelated fallback font. Variable fonts and exact weight matches still use the
existing font database ordering.

## Optical-size overrides

`FontSystem::set_font_optical_size` lets the application configure a fixed `opsz`
value for an installed font face before text layout. Shaping metrics, HarfRust,
and both Swash raster and outline paths use the same normalized variation
coordinates. Faces without an explicit optical-size override retain the upstream
rendering path. Swash images and outlines share one scaler builder so their
variation settings stay consistent.

## Change an optical-size setting

Changing a setting evicts cached font instances and shaping scratch data. Callers
must reshape existing text and clear external raster caches if they change it
after rendering has begun. The application configures its macOS UI faces during
initial font-system construction.

## Related documentation

- [Documentation index](../../docs/README.md)
- [Rendering development](../../docs/development/rendering.md)
- [Dependency update process](../README.md#updates)
