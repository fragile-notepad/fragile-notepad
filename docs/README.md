[Project overview](../README.md)

# Fragile Notepad documentation

## Use the editor

| Goal | Read |
| --- | --- |
| Download the editor and open a file | [Get started](getting-started.md) |
| Look up command-line options, settings locations, or environment variables | [Runtime and build options](reference/runtime-options.md) |
| Add a file-manager context menu | [Native components](../components/README.md) |

## Develop and distribute

| Goal | Read |
| --- | --- |
| Set up and run a development build | [Build from source](development/building.md) |
| Select checks for a change | [Test and validate](development/testing.md) |
| Compile icons or regenerate illustrations and font profiles | [Generate assets](development/assets.md) |
| Diagnose rendering or measure drawing time | [Test and profile rendering](development/rendering.md) |
| Create portable archives and macOS app bundles | [Package the editor](development/packaging.md) |
| Prepare a contribution | [Contributing](../CONTRIBUTING.md) |
| Write or reorganize a documentation page | [Writing documentation](development/documentation.md) |

## Understand the architecture

| Topic | Read |
| --- | --- |
| State ownership, command routing, windows, and shutdown | [Application architecture](architecture/application.md) |
| Software startup, rendering policy, and Vulkan handoff | [Rendering architecture](architecture/rendering.md) |
| GPU resource ownership, retained painting, and damage | [Vulkan renderer](architecture/vulkan.md) |

## Resource and dependency reference

| Topic | Read |
| --- | --- |
| Outline parser and folding configuration | [Syntax configuration](../assets/syntax/README.md) |
| Icon families, source formats, and notices | [Icons](../assets/icons/README.md) |
| Branding and illustration sources | [Illustrations](../assets/illustrations/README.md), [Bunny artwork](../assets/illustrations/bunny/README.md) |
| Dependency versions, licenses, and local patches | [Vendored dependencies](../vendor/README.md) |
| Windows Explorer integration | [Windows shell component](../components/windows-shell/README.md) |
| Nautilus and Dolphin integration | [Linux shell components](../components/linux-shell/README.md) |
| Finder Services integration | [macOS shell component](../components/macos-shell/README.md) |
