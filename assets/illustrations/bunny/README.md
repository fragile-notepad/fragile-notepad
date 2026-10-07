# Bunny artwork

`app.svg` is the supplied BunnyNotebook master; `title-bar.svg` removes its
background. Sources and derivatives use [the artwork license](../LICENSE).

`python scripts/generate_app_icons.py` (included in standard asset generation)
uses resvg and Pillow to produce embedded RGBA and
`target/app-icons/app.ico`, `app.icns`, and `app.png`.
Generated files are ignored by Git.

Preview the About/title-bar animation with
`cargo run --locked --example preview_branding` → `target/bunny-review/`.
Add `-- --vulkan` for `target/bunny-review-vulkan/`.
