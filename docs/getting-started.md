[Documentation](README.md)

# Get started

Download Fragile Notepad, open a file, and adjust the editor's preferences.
Windows and Linux are supported; macOS support is experimental.

## Download and launch

1. Open [Releases](https://github.com/fragile-notepad/fragile-notepad/releases)
   and choose the archive for your platform. Nightly builds are prereleases.
2. Extract the complete archive into a directory you can keep in place.
3. Launch the editor using the entry below.

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

## Related topics

- [Runtime and build options](reference/runtime-options.md)
- [Optional file-manager integrations](../components/README.md)
