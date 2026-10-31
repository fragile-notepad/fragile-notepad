"""Wrap a prepared portable macOS package in its named application bundle."""

import argparse
import json
import os
import plistlib
import shlex
import shutil
import subprocess
import sys
from pathlib import Path


APP_NAME = "Fragile Notepad"
REPO_ROOT = Path(__file__).resolve().parent.parent
MACH_O_MAGIC = (
    b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf",
    b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca",
    b"\xca\xfe\xba\xbf", b"\xbf\xba\xfe\xca",
)


def validate_inputs(package, icon, launcher_binary):
    executable = package / "libexec/fragile-notepad"
    launcher = package / "fragile-notepad"
    if not executable.is_file() or not launcher.is_file():
        raise ValueError("package must contain the launcher and libexec/fragile-notepad")
    if icon is not None and not icon.is_file():
        raise ValueError(f"missing generated application icon: {icon}")
    if launcher_binary is not None:
        if not launcher_binary.is_file():
            raise ValueError(f"missing native launcher: {launcher_binary}")
        with launcher_binary.open("rb") as binary:
            if binary.read(4) not in MACH_O_MAGIC:
                raise ValueError("--launcher-binary must be a native Mach-O executable")
    elif sys.platform != "darwin":
        raise ValueError("cross packaging requires --launcher-binary (build scripts/macos-app-launcher.c)")


def copy_runtime(package, runtime):
    runtime.mkdir()
    for name in ("lib", "libexec", "share", "licenses", "font-licenses"):
        source = package / name
        if source.is_dir():
            shutil.copytree(source, runtime / name, symlinks=True)
    for name in ("LICENSE", "ICON-NOTICES.txt", "FILE-TYPE-ICON-NOTICES.txt", "FONT-NOTICES.txt"):
        source = package / name
        if source.is_file():
            shutil.copy2(source, runtime / name)
    # Keep the same runtime setup and arguments in the portable launcher. Always
    # normalize its line endings when packaging from a Windows checkout.
    launcher = package / "fragile-notepad"
    (runtime / "fragile-notepad").write_bytes(launcher.read_bytes().replace(b"\r\n", b"\n"))
    (runtime / "fragile-notepad").chmod(0o755)
    (runtime / "libexec/fragile-notepad").chmod(0o755)


def install_native_launcher(destination, launcher_binary, compiler, minimum_system_version):
    if launcher_binary is not None:
        shutil.copy2(launcher_binary, destination)
    else:
        subprocess.run([
            *compiler,
            "-std=c11", "-Os", "-Wall", "-Wextra", "-Werror",
            f"-mmacosx-version-min={minimum_system_version}",
            str(Path(__file__).with_name("macos-app-launcher.c")),
            "-o", str(destination),
        ], check=True)
    destination.chmod(0o755)


def document_types(icon_directory):
    entries = json.loads((REPO_ROOT / "assets/icons/file-types/catalog.json").read_text())
    documents = []
    for entry in entries:
        if not entry["extensions"]:
            continue
        source = icon_directory / f"{entry['id']}.icns"
        if not source.is_file():
            raise ValueError(f"missing generated document icon: {source}")
        documents.append((source, {
            "CFBundleTypeName": f"{entry['name']} document",
            "CFBundleTypeExtensions": [extension.removeprefix(".") for extension in entry["extensions"]],
            "CFBundleTypeIconFile": f"FileType-{entry['id']}.icns",
            "CFBundleTypeRole": "Editor",
            "LSHandlerRank": "Alternate",
        }))
    return documents


def write_bundle_metadata(contents, version, minimum_system_version, icon, documents):
    metadata = {
        "CFBundleIdentifier": "com.fragilenotepad.app",
        "CFBundleName": APP_NAME,
        "CFBundleDisplayName": APP_NAME,
        "CFBundleExecutable": APP_NAME,
        "CFBundlePackageType": "APPL",
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleVersion": version,
        "CFBundleShortVersionString": version,
        "LSMinimumSystemVersion": minimum_system_version,
        "NSHighResolutionCapable": True,
        "CFBundleDocumentTypes": [metadata for _, metadata in documents],
    }
    for source, document in documents:
        shutil.copy2(source, contents / "Resources" / document["CFBundleTypeIconFile"])
    if icon is not None:
        shutil.copy2(icon, contents / "Resources/FragileNotepad.icns")
        metadata["CFBundleIconFile"] = "FragileNotepad.icns"
    with (contents / "Info.plist").open("wb") as output:
        plistlib.dump(metadata, output, sort_keys=False)
    (contents / "PkgInfo").write_bytes(b"APPL????")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    parser.add_argument("bundle", type=Path)
    parser.add_argument("--icon", type=Path)
    parser.add_argument("--file-type-icons", type=Path,
                        default=REPO_ROOT / "target/file-type-icons/macos",
                        help="directory of generated document .icns files")
    parser.add_argument("--version", default="0.1.0")
    parser.add_argument("--launcher-binary", type=Path,
                        help="prebuilt native macos-app-launcher for cross packaging")
    args = parser.parse_args()
    try:
        package = args.package.resolve(strict=True)
        bundle = args.bundle.resolve()
        validate_inputs(package, args.icon, args.launcher_binary)
        documents = document_types(args.file_type_icons)
        compiler = []
        if args.launcher_binary is None:
            compiler = shlex.split(os.environ.get("CC", "cc"))
            if not compiler or shutil.which(compiler[0]) is None:
                raise ValueError("CC must name an available C compiler")
    except (OSError, ValueError) as error:
        parser.error(str(error))

    minimum_system_version = os.environ.get("MACOSX_DEPLOYMENT_TARGET", "11.0")
    # Refuse an existing destination so old files never leak into the bundle.
    bundle.mkdir()
    contents = bundle / "Contents"
    macos = contents / "MacOS"
    resources = contents / "Resources"
    macos.mkdir(parents=True)
    resources.mkdir()
    copy_runtime(package, resources / "runtime")
    install_native_launcher(
        macos / APP_NAME, args.launcher_binary, compiler, minimum_system_version,
    )
    write_bundle_metadata(contents, args.version, minimum_system_version, args.icon, documents)
    print(bundle)


if __name__ == "__main__":
    main()
