# Linux file manager context menus

Standalone native C components for **Edit with Fragile Notepad**:
Nautilus API 4 (GNOME Files 43+) and KDE Dolphin (Frameworks 5/6).
Currently unused; no shell scripts or automatic registration.

Build on Linux, including WSL. Dependencies: C11 compiler, CMake 3.20+,
pkg-config, GLib/GIO development files 2.46+ (`gio-2.0`), and Nautilus
API 4 development files (`libnautilus-extension-4`). Match Nautilus's architecture.

Configure the stable absolute path of the separately installed editor.
From the repository root:

```text
cmake -S components/linux-shell -B target/linux-shell -DCMAKE_INSTALL_PREFIX=/usr/local -DFRAGILE_NOTEPAD_EXECUTABLE=/usr/local/bin/fragile-notepad
cmake --build target/linux-shell --parallel 1
```

Optional flags: `-DFRAGILE_BUILD_NAUTILUS=OFF` for Dolphin only; `-DBUILD_TESTING=OFF` to omit tests.

Install explicitly after the editor is present at its configured path:

```text
sudo cmake --install target/linux-shell
```

Installation destinations:

- Nautilus: `fragile-notepad-nautilus.so` in pkg-config's `extensiondir`; override with `FRAGILE_NAUTILUS_INSTALL_DIR` only to a directory Nautilus searches.
- Dolphin launcher: `FRAGILE_SHELL_LAUNCHER`, default `/usr/local/libexec/fragile-notepad-shell-launcher` for this prefix.
- Dolphin menu: `fragile-notepad.desktop` in `share/kio/servicemenus` under the prefix.

For Frameworks before 5.85, add `-DFRAGILE_DOLPHIN_INSTALL_DIR=share/kservices5/ServiceMenus`.
Set editor/launcher paths before building; changing the prefix later does not update cached absolute paths.

Uninstall by removing those three installed artifacts (only two with Nautilus disabled).
Restart the file manager or sign out after installation or removal.
