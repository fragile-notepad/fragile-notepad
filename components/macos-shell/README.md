[Documentation](../../docs/README.md) / [File-manager integrations](../README.md)

# macOS Finder integration

This native `.service` bundle adds **Edit with Fragile Notepad** to Finder's
Services menu. It accepts between 1 and 4,096 local regular files, including
symlinks to files.

The component is independent of normal application builds, CI, and packages.
Building it does not install or activate the service.

## Requirements

- macOS 11 or later.
- Xcode Command Line Tools and CMake 3.20+.
- A separately installed editor or an existing portable launcher at a stable
  absolute path.

## Build and install

1. Configure from the repository root. Replace the example path with the editor
   executable or existing portable launcher:

   ```sh
   cmake -S components/macos-shell -B target/macos-shell -DCMAKE_BUILD_TYPE=Release "-DFRAGILE_NOTEPAD_EXECUTABLE=/absolute/path/to/fragile-notepad"
   ```

2. Build with one job:

   ```sh
   cmake --build target/macos-shell --parallel 1
   ```

   The build creates `Fragile Notepad.service` under `target/macos-shell/`.

3. Install for the current user:

   ```sh
   cmake --install target/macos-shell --prefix "$HOME/Library/Services"
   ```

   The installed bundle is `~/Library/Services/Fragile Notepad.service`.

4. Sign out and back in to refresh Services. Enable the action under
   **Keyboard Shortcuts > Services** if it is not visible.

## Configuration

| CMake option | Purpose |
| --- | --- |
| `FRAGILE_NOTEPAD_EXECUTABLE` | Required absolute path embedded in the service bundle |
| `CMAKE_OSX_DEPLOYMENT_TARGET` | Defaults to `11.0` |

Reconfigure, rebuild, and reinstall the bundle after moving the editor.

## Remove the integration

Remove `~/Library/Services/Fragile Notepad.service`, then sign out and back in to
refresh Services.

## Related documentation

- [File-manager integrations](../README.md)
- [macOS application packaging](../../docs/development/packaging.md)
