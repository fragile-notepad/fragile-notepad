# File-manager integration

Standalone, unused components; excluded from normal app builds, CI, and packages.
Building does not activate them. Installer integration, including NSIS, is deferred.

| Component | Integration |
| --- | --- |
| [Windows](windows-shell/README.md) | Windows 11+ main menu; earlier Windows classic menu |
| [Linux](linux-shell/README.md) | Nautilus 4 extension and Dolphin service menu |
| [macOS](macos-shell/README.md) | Finder Services bundle |

Each opens 1–4096 selected files through native arguments (`--` followed by paths).
Build and install the editor separately. No shell scripts are included.
