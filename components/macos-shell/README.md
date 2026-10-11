# macOS Finder Services

Standalone `.service` bundle providing **Edit with Fragile Notepad** in Finder's
Services menu. Accepts 1–4096 local regular files, including symlinks to files.

Requires macOS 11+, Xcode Command Line Tools, and CMake 3.20+. Use a stable absolute
path to the separately installed editor or its existing portable launcher.

```text
cmake -S components/macos-shell -B target/macos-shell -DCMAKE_BUILD_TYPE=Release "-DFRAGILE_NOTEPAD_EXECUTABLE=/absolute/path/to/fragile-notepad"
cmake --build target/macos-shell --parallel 1
```

Install for the current user:

```text
cmake --install target/macos-shell --prefix "$HOME/Library/Services"
```

Sign out and back in to refresh Services; enable the action under Keyboard
Shortcuts > Services if needed. To uninstall, remove
`~/Library/Services/Fragile Notepad.service` and refresh again.
Rebuild after moving the editor.
