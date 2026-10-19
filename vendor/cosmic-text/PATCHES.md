# Local changes to cosmic-text 0.19.0

The upstream package source, manifests, tests, fixtures, and license files are
preserved here. The application supplies fonts from the operating system; this
patch does not contain or generate Apple font data.

`FontSystem::set_font_optical_size` lets the application configure a fixed `opsz`
value for an installed font face before text layout. Shaping metrics, HarfRust,
and both Swash raster and outline paths use the same normalized variation
coordinates. Faces without an explicit optical-size override retain the upstream
rendering path. Swash images and outlines share one scaler builder so their
variation settings stay consistent.

Changing a setting evicts cached font instances and shaping scratch data. Callers
must reshape existing text and clear external raster caches if they change it
after rendering has begun. The application configures its macOS UI faces during
initial font-system construction.
