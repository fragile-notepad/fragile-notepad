"""Extract and launch a release archive using an isolated configuration directory."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import zipfile


FRAME_MARKER = "FRAGILE_NOTEPAD_FIRST_FRAME_READY_MS="


def extract_archive(archive, destination):
    if archive.name.endswith(".zip"):
        with zipfile.ZipFile(archive) as package:
            for member in package.infolist():
                target = (destination / member.filename).resolve()
                if not target.is_relative_to(destination.resolve()):
                    raise ValueError(f"Archive member escapes package directory: {member.filename}")
            package.extractall(destination)
    else:
        with tarfile.open(archive, "r:gz") as package:
            # The data filter also rejects unsafe links and special files.
            package.extractall(destination, filter="data")


def package_launchers(package, platform):
    if platform == "win32":
        launchers = [package / "fragile-notepad.exe"]
    elif platform == "darwin":
        launchers = [
            package / "fragile-notepad",
            package / "Fragile Notepad.app/Contents/MacOS/Fragile Notepad",
        ]
    else:
        launchers = [package / "fragile-notepad"]
    for launcher in launchers:
        if not launcher.is_file():
            raise ValueError(f"Missing packaged launcher: {launcher}")
    if platform == "darwin":
        for root in (package, package / "Fragile Notepad.app/Contents/Resources/runtime"):
            for filename in ("lib/libvulkan.1.dylib", "lib/libvulkan.dylib",
                             "lib/libMoltenVK.dylib", "libexec/fragile-notepad"):
                if not (root / filename).is_file():
                    raise ValueError(f"Missing packaged file: {root / filename}")
            manifest = root / "share/vulkan/icd.d/MoltenVK_icd.json"
            library = (manifest.parent / json.loads(manifest.read_text())["ICD"]["library_path"]).resolve()
            if not library.is_relative_to(root.resolve()) or not library.is_file():
                raise ValueError(f"Invalid Vulkan library path in {manifest}")
    return launchers


def run_checked(command, environment, timeout):
    result = subprocess.run(
        command, env=environment, capture_output=True, text=True,
        encoding="utf-8", errors="replace", timeout=timeout, check=False,
        creationflags=subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"Command failed ({result.returncode}): {command!r}\n"
            f"{result.stdout}\n{result.stderr}"
        )
    return result.stdout


def check_launcher(command, environment, timeout):
    for argument, marker in [("--version", "fragile-notepad "), ("--help", "Usage:")]:
        output = run_checked([*command, argument], environment, timeout)
        if marker not in output:
            raise RuntimeError(f"Packaged launcher did not report {argument}: {output}")
    startup_environment = dict(environment)
    startup_environment.update({
        "FRAGILE_NOTEPAD_STARTUP_PROBE": "1",
        "FRAGILE_NOTEPAD_EXIT_AFTER_FRAME": "1",
        "FRAGILE_NOTEPAD_RENDER_BACKEND": "software",
    })
    output = run_checked([*command, "--no-session"], startup_environment, timeout)
    if FRAME_MARKER not in output:
        raise RuntimeError(f"Packaged editor exited without rendering its first frame: {output}")
    print(output.strip())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("--timeout", type=float, default=60,
                        help="seconds allowed for each launcher command (default: 60)")
    args = parser.parse_args()
    try:
        if args.timeout <= 0:
            raise ValueError("--timeout must be positive")
        with tempfile.TemporaryDirectory(prefix="fragile-package-check-") as temporary:
            root = Path(temporary)
            package = root / "extracted package"
            package.mkdir()
            extract_archive(args.archive.resolve(strict=True), package)
            config = root / "config"
            config.mkdir()
            environment = dict(os.environ)
            environment.update({
                "APPDATA": str(config), "LOCALAPPDATA": str(config),
                "XDG_CONFIG_HOME": str(config), "XDG_CACHE_HOME": str(root / "cache"),
                "HOME": str(root),
            })
            for key in ("FRAGILE_NOTEPAD_STARTUP_PROBE", "FRAGILE_NOTEPAD_EXIT_AFTER_FRAME"):
                environment.pop(key, None)
            for launcher in package_launchers(package, sys.platform):
                print(f"Checking {launcher}", flush=True)
                check_launcher([str(launcher)], environment, args.timeout)
    except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.TimeoutExpired,
            tarfile.TarError, zipfile.BadZipFile) as error:
        print(f"Package check failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
