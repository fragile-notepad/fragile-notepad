# Windows Explorer context menu

Standalone native DLL adding **Edit with Fragile Notepad** for selected files.
Currently unused by the editor, root Cargo builds, CI, and portable packages.
NSIS integration is deferred; building does not register the component.

| Windows | Registration | Menu |
| --- | --- | --- |
| Windows 11+ | Signed sparse MSIX package | Main context menu |
| Previous versions | Current-user COM class and `ExplorerCommandHandler` | Classic menu |

The Rust/editor Windows 10+ baseline applies.

Build from the repository root on Windows:

```text
cargo build --manifest-path components/windows-shell/Cargo.toml --release --locked -j 1 --target-dir target/windows-shell
```

Output: `target/windows-shell/release/fragile_notepad_shell.dll`.
Place it beside `fragile-notepad.exe` in a stable drive/UNC installation directory.
Build for Explorer's native OS architecture; an explicit Cargo `--target`
adds the target name before `release/`.

Explicitly invoke the exported `DllRegisterServer` using a host matching the DLL
architecture. It detects the OS and selects the registration path automatically:
earlier Windows uses `HKCU\Software\Classes` and `*\shell\FragileNotepad.Edit`;
Windows 11+ deploys the signed sparse package and removes this classic verb.
Registration applies to the current user and needs no elevation.

For Windows 11+, prepare [AppxManifest.xml](AppxManifest.xml) and place the signed
`FragileNotepad.ShellIntegration.msix` beside the DLL, with its `Assets` files.
Match the manifest architecture to the DLL and its publisher to the signing
certificate. Set `FRAGILE_SHELL_PUBLISHER` when building to that same publisher
(default: `CN=Fragile Notepad`); keep package identities consistent across updates.

Invoke exported `DllUnregisterServer` before removing the DLL, EXE, or package
assets. It removes this component's modern package and classic registration.
Explorer may retain the loaded DLL until sign-out.
