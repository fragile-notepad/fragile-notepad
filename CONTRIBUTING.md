# Contributing to Fragile Notepad

Use this guide to prepare a change and choose the checks that apply to it.

## Set up a development build

Follow [Build from source](docs/development/building.md), then use the
[documentation index](docs/README.md) to find the relevant subsystem guide.

| Area | Starting point |
| --- | --- |
| Application workflows, windows, and sessions | [Application architecture](docs/architecture/application.md) |
| Software and Vulkan rendering | [Rendering architecture](docs/architecture/rendering.md) |
| Icons, illustrations, and font profiles | [Generate assets](docs/development/assets.md) |
| Function navigation and folding rules | [Syntax configuration](assets/syntax/README.md) |
| Platform file-manager integrations | [Native components](components/README.md) |
| Local dependency patches | [Vendored dependencies](vendor/README.md) |

## Prepare a change

1. Check the relevant source and documentation before changing behavior.
2. Keep the change focused. Update its canonical documentation page when commands,
   configuration, or behavior change.
3. Run the [focused checks](docs/development/testing.md#choose-focused-checks)
4. Describe the problem, resulting behavior, and validation in the pull request.
   Include screenshots when a visual change needs review.

See [Writing documentation](docs/development/documentation.md) for page placement,
wording, and link conventions. See [Packaging](docs/development/packaging.md) for
release artifacts and notices.
