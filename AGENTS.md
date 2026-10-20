# Fragile Notepad

Fragile Notepad is a desktop text editor for notes and source files, written in
Rust 2024 with Iced. Windows and Linux are supported; macOS support is
experimental. Documents use Ropey. The editor starts with tiny-skia software
rendering; the default `hybrid-rendering` feature allows a later handoff to
wgpu's Vulkan backend. `--no-default-features` builds exclude Vulkan.

These instructions apply throughout the repository.

## Source layout

- `src/core/`: document, workspace, settings, and session models.
- `src/app/`: command routing, workflow coordination, editor windows, and shutdown.
- `src/editor/`: text layout, editing, wrapping, folding, and outline parsing.
- `src/ui/`: widgets and presentation. Widgets emit messages; application
  workflows own mutations and services return typed results.
- `src/services/`, `src/ipc/`, and `src/platform/`: external operations,
  single-instance communication, and platform support.
- `tests/`: integration and rendering tests. `examples/` contains previews and
  profiling tools.
- `assets/`, `scripts/`, and `build.rs`: source artwork, syntax resources, asset
  generation, font preparation, validation, and packaging.
- [components/](components/README.md): optional native file-manager integrations,
  built and installed separately from the editor.
- [vendor/](vendor/README.md): local dependency sources and patch records.

## Documentation

[README.md](README.md) introduces the product.
[docs/README.md](docs/README.md) is the documentation index, and
[CONTRIBUTING.md](CONTRIBUTING.md) describes the contribution workflow.

Read the guide for the area you are changing:

- [Building](docs/development/building.md) and
  [testing](docs/development/testing.md) cover setup and validation commands.
- [Application architecture](docs/architecture/application.md) describes state
  ownership, message routing, window transfers, and asynchronous work.
- [Rendering architecture](docs/architecture/rendering.md), the
  [Vulkan renderer](docs/architecture/vulkan.md), and
  [rendering checks](docs/development/rendering.md) cover backend changes.
- [Asset generation](docs/development/assets.md) and the
  [syntax reference](assets/syntax/README.md) cover embedded resources.
- [Runtime options](docs/reference/runtime-options.md) records arguments,
  environment variables, and settings locations.
- [Packaging](docs/development/packaging.md) describes release artifacts and
  notices. Component and asset READMEs document their local requirements.

## Working agreements

- This computer is the user's active development machine. Keep the IDE and WSL
  responsive and leave capacity for the user's other work.
- Prefer focused tests and incremental checks. Use existing caches; avoid
  reproducing an entire CI pipeline locally unless the task requires it or the
  user requests it.
- Run only one resource-intensive build or test command at a time, across both
  Windows and WSL. Use one Cargo build job (`-j 1` or `CARGO_BUILD_JOBS=1`) and one
  test thread for graphics or other memory-intensive tests.
- If a build runs out of memory or disrupts the IDE or WSL, stop heavy work,
  explain what happened, and use a smaller workload before continuing. Do not
  immediately retry the same resource-intensive operation.

## Making changes

- Read the relevant source and guide before editing. Follow the ownership and
  lifecycle rules in the architecture documentation.
- Keep changes within the requested scope and preserve unrelated work in the
  checkout.
- Edit asset sources and regenerate affected outputs using the documented
  scripts. Cargo prepares font profiles during builds; Python is a build tool,
  not an application runtime dependency.
- Record vendored dependency changes in the existing `LOCAL_CHANGES.md` or
  `PATCHES.md`. Keep provenance and upstream notices with the dependency.
- Follow each asset collection's license. The code's BSD license does not cover
  all artwork; see the [license overview](README.md#license).
- Select checks from the testing guide for the changed behavior. For
  documentation-only changes, check relative links, heading anchors, paths, and
  Markdown formatting, then run `git diff --check`. No application build is
  needed.

## Writing documentation

Use the [writing guide](docs/development/documentation.md) when editing docs.
Update the page that owns a changed behavior or command. Link to that page from
other locations; update incoming links when moving a page or renaming a heading.

Write direct instructions with exact commands, defaults, and output paths. Verify
claims against source or workflows. Keep benchmark conditions and limitations
with the measurements.

Choose the structure each page needs. Omit introductions that repeat the title,
duplicated navigation, and automatic summaries or related-link sections. Use
tables for comparisons and option reference, and numbered steps for procedures.
Avoid promotional filler, stock contrast slogans, and padded three-part phrases.
