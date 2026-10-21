# Contributing to Fragile Notepad

## Set up a development build

Follow [Build from source](docs/development/building.md).

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
2. Keep the change focused. Update its documentation when commands, configuration,
   or behavior change. Follow the [writing guide](docs/development/documentation.md).
3. Run the [focused checks](docs/development/testing.md#choose-focused-checks).
4. Describe the problem, resulting behavior, and validation in the pull request.
   Include screenshots when a visual change needs review.
