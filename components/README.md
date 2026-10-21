[Documentation](../docs/README.md)

# File-manager integrations

These optional native components add **Edit with Fragile Notepad** to the
platform's file manager. Each launches the separately installed editor with
`--` followed by the selected file paths. Selections must contain between 1 and
4,096 files.

## Build and installation

Application builds, CI, and [packages](../docs/development/packaging.md) exclude
these components. Build and install them separately using the platform guides
below.

| Platform | Component | Installation guide |
| --- | --- | --- |
| Windows | Explorer command DLL; Windows 11+ main menu or earlier Windows classic menu | [Windows Explorer](windows-shell/README.md) |
| Linux | Nautilus API 4 extension and Dolphin service menu | [Linux file managers](linux-shell/README.md) |
| macOS | Finder Services bundle | [macOS Finder](macos-shell/README.md) |

Build and install the [editor](../docs/development/building.md) before installing
a component. Keep the editor at a stable path; moving it requires updating the
integration.
