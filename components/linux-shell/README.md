[Documentation](../../docs/README.md) / [File-manager integrations](../README.md)

# Linux file-manager integration

These native C components add **Edit with Fragile Notepad** to Nautilus
(GNOME Files 43+, API 4) and KDE Dolphin (Frameworks 5/6).

## Requirements

- Linux, including WSL for building.
- A C11 compiler, CMake 3.20+, and pkg-config.
- GLib/GIO development files 2.46+ (`gio-2.0`).
- For Nautilus, API 4 development files (`libnautilus-extension-4`) matching the
  installed file manager's architecture.
- A separately installed editor at a stable absolute path.

## Build and install

1. Configure from the repository root. Set the executable path to the installed
   editor before building:

   ```sh
   cmake -S components/linux-shell -B target/linux-shell -DCMAKE_INSTALL_PREFIX=/usr/local -DFRAGILE_NOTEPAD_EXECUTABLE=/usr/local/bin/fragile-notepad
   ```

2. Build with one job:

   ```sh
   cmake --build target/linux-shell --parallel 1
   ```

   The build produces the native Dolphin launcher and service-menu entry, plus
   `fragile-notepad-nautilus.so` when Nautilus support is enabled. Build files
   remain under `target/linux-shell/`.

3. Ensure the editor exists at the configured path, then install:

   ```sh
   sudo cmake --install target/linux-shell
   ```

4. Restart the file manager or sign out to load the integration.

## Configuration

Pass these CMake options during the configure step.

| Option | Default or purpose |
| --- | --- |
| `FRAGILE_NOTEPAD_EXECUTABLE` | Absolute editor path; defaults to the install prefix's `bin/fragile-notepad` |
| `FRAGILE_BUILD_NAUTILUS` | `ON`; set `OFF` to build Dolphin support only |
| `FRAGILE_NAUTILUS_INSTALL_DIR` | Nautilus's pkg-config `extensiondir`; override only with a directory Nautilus searches |
| `FRAGILE_SHELL_LAUNCHER` | Absolute Dolphin launcher path; `/usr/local/libexec/fragile-notepad-shell-launcher` with the prefix above |
| `FRAGILE_DOLPHIN_INSTALL_DIR` | `share/kio/servicemenus` under the install prefix |
| `BUILD_TESTING` | `ON`; set `OFF` to omit component tests from the build |

For KDE Frameworks before 5.85, set
`-DFRAGILE_DOLPHIN_INSTALL_DIR=share/kservices5/ServiceMenus`.

Editor and launcher paths are cached as absolute paths at configuration time.
Changing only the installation prefix later does not update them; reconfigure
the paths before rebuilding.

## Installed files and removal

| Artifact | Destination |
| --- | --- |
| `fragile-notepad-nautilus.so` | `FRAGILE_NAUTILUS_INSTALL_DIR`; omitted when Nautilus is disabled |
| Native Dolphin launcher | `FRAGILE_SHELL_LAUNCHER` |
| `fragile-notepad.desktop` | `FRAGILE_DOLPHIN_INSTALL_DIR` under the install prefix |

To uninstall, remove those three installed artifacts, or the two Dolphin
artifacts if Nautilus was disabled. Restart the file manager or sign out after
removal.
