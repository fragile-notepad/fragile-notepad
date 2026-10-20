[Documentation](../docs/README.md)

# File-manager integrations

These optional native components add **Edit with Fragile Notepad** to the
platform's file manager. Each launches the separately installed editor with
`--` followed by the selected file paths. Selections must contain between 1 and
4,096 files.

## Integration status

The components are independent of the application build. Normal Cargo builds,
CI, and application packages do not build or install them. Building a component
does not activate it; follow its installation or registration procedure.
Installer integration, including NSIS, is deferred.

| Platform | Component | Installation guide |
| --- | --- | --- |
| Windows | Explorer command DLL; Windows 11+ main menu or earlier Windows classic menu | [Windows Explorer](windows-shell/README.md) |
| Linux | Nautilus API 4 extension and Dolphin service menu | [Linux file managers](linux-shell/README.md) |
| macOS | Finder Services bundle | [macOS Finder](macos-shell/README.md) |

These integrations use native launch arguments and include no shell-script
launchers. Build and install the editor before installing a component. Keep the
editor at a stable path; moving it requires updating the integration.

## Related documentation

- [Build the application](../docs/development/building.md)
- [Package the application](../docs/development/packaging.md)
