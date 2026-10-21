[Documentation](../../docs/README.md) / [File-manager integrations](../README.md)

# Windows Explorer integration

This native DLL adds **Edit with Fragile Notepad** to Explorer for selected files.

| Windows version | Registration method | Menu location |
| --- | --- | --- |
| Windows 11+ | Signed sparse MSIX package | Main context menu |
| Earlier supported Windows | Current-user COM class and `ExplorerCommandHandler` | Classic context menu |

## Requirements

- Windows 10 or later, matching the editor's Rust and operating-system baseline.
- Rust and the native build tools described in [Build the application](../../docs/development/building.md).
- An installed `fragile-notepad.exe` in a stable drive or UNC directory.
- A DLL build and registration host matching Explorer's native architecture.
- On Windows 11+, a signed sparse MSIX package and the manifest's `Assets` files.

## Build and register

1. Build from the repository root on Windows:

   ```powershell
   cargo build --manifest-path components/windows-shell/Cargo.toml --release --locked -j 1 --target-dir target/windows-shell
   ```

   The DLL is written to
   `target/windows-shell/release/fragile_notepad_shell.dll`. If you supply a
   Cargo `--target`, its target name appears before `release/` in that path.

2. Place `fragile_notepad_shell.dll` beside the installed `fragile-notepad.exe`.
   On Windows 11+, prepare the sparse package as described below before
   registration.

3. Invoke the exported `DllRegisterServer` with a host matching the DLL's
   architecture. For example, use the matching `regsvr32.exe`:

   ```powershell
   regsvr32.exe "C:\path\to\installation\fragile_notepad_shell.dll"
   ```

   Registration applies to the current user and requires no elevation. The DLL
   detects the Windows version and selects the registration method. Earlier
   Windows uses `HKCU\Software\Classes` and
   `*\shell\FragileNotepad.Edit`. Windows 11+ deploys the signed sparse package
   and removes this component's classic verb.

## Windows 11 package configuration

Use [AppxManifest.xml](AppxManifest.xml) as the package manifest. Place the signed
`FragileNotepad.ShellIntegration.msix` beside the DLL, together with the manifest's
`Assets` files.

| Setting | Required value |
| --- | --- |
| Manifest `ProcessorArchitecture` | The DLL's native Explorer architecture |
| Manifest `Publisher` | The package-signing certificate's publisher |
| Build environment `FRAGILE_SHELL_PUBLISHER` | The same publisher; defaults to `CN=Fragile Notepad` |
| Package identity | Consistent across updates |

Set `FRAGILE_SHELL_PUBLISHER` before the Cargo build when using a publisher other
than the default. Prepare and sign the package manually before registration.

## Remove the integration

1. Invoke `DllUnregisterServer` with a matching registration host before removing
   the installed files:

   ```powershell
   regsvr32.exe /u "C:\path\to\installation\fragile_notepad_shell.dll"
   ```

2. Remove the DLL and package assets after unregistration succeeds. The
   unregistration routine removes this component's modern package and classic
   registration. Explorer may retain the loaded DLL until sign-out.
