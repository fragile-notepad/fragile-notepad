[Documentation](README.md)

# Get started

Windows and Linux are supported; macOS support is experimental.

## Download and launch

Open [Releases](https://github.com/fragile-notepad/fragile-notepad/releases)
and choose an installer or portable archive for your platform. Nightly builds are
prereleases.

| Platform | Install |
| --- | --- |
| Windows x64 | Run `*-setup.exe`, then open Fragile Notepad from the Start menu. Setup requires administrator access. |
| Debian / Ubuntu | Run `sudo apt install ./fragile-notepad-*.deb`. |
| Fedora | Run `sudo dnf install ./fragile-notepad-*.rpm`. |
| openSUSE | Run `sudo zypper install ./fragile-notepad-*.rpm`. |
| macOS | Open the `.dmg` for Apple Silicon (`arm64`) or Intel (`x86_64`), then drag Fragile Notepad to Applications. |

Choose Fragile Notepad in the OS's default-app settings for text and source files.
See [package details](development/packaging.md) for document icons, Linux
requirements, and unsigned downloads.

For a portable archive, extract the complete archive into a directory you can
keep in place, then launch the editor:

| Platform | Launch |
| --- | --- |
| Windows x64 | Open `fragile-notepad.exe`. |
| Linux x64 | Run `./fragile-notepad` from the extracted directory in a graphical desktop session. |
| macOS | Open `Fragile Notepad.app` or run the adjacent `./fragile-notepad` launcher. |

Keep the macOS archive's runtime files together. Its launcher configures the
bundled Vulkan loader and MoltenVK libraries.

To compile the editor instead, follow [Build from source](development/building.md).

## Open and save a file

1. Choose **File > Open...** and select a text or source file. The document opens
   in a tab.
2. Edit the document, then choose **File > Save**. For a new document, choose
   **File > New**, enter text, and save it to a path.
3. Use **File > Save As...** to save under another name.

**Decoding errors** means some bytes could not be read using the selected encoding.
Saving to the original file and autosave are blocked. Reopen with the correct
encoding from the **Encoding** menu, or use **File > Save As...** to save the
displayed text to a different file.

You can also pass file paths on the command line. These examples run from the
extracted archive directory.

Windows PowerShell:

```powershell
.\fragile-notepad.exe .\notes.txt
```

Linux or macOS:

```sh
./fragile-notepad ./notes.txt
```

A second launch forwards files to the running instance. Relative paths use the
launching shell's working directory. See [Command line](reference/runtime-options.md#command-line)
for multiple files and filenames that begin with a hyphen.

## Change preferences

Open **Settings > Preferences...** to configure appearance, editing, display,
search, and shortcuts. The **Shortcuts** category shows the configured key bindings.

The editor restores the previous session by default. To start without restoring
it, close the running instance and launch with `--no-session`.
See [Settings and session files](reference/runtime-options.md#settings-and-session-files)
for storage locations.
