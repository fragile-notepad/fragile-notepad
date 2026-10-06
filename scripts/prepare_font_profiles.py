#!/usr/bin/env python3
"""Prepare measured font profiles and, when necessary, a pinned CJK fallback.

Build outputs are Rust expressions; candidate JSON, reports, dependency venvs,
and downloaded fonts stay in the ignored cache. FRAGILE_FONT_OFFLINE=1 forbids
network access and dependency installation, while permitting verified cache use.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from typing import Any
import urllib.request

import font_families as catalog
import generate_font_profiles as generator
from generate_font_profiles import Face, atomic_write


SCRIPT_DIR = Path(__file__).resolve().parent
REPOSITORY = SCRIPT_DIR.parent
NOTO_COMMIT = "523d033d6cb47f4a80c58a35753646f5c3608a78"  # Upstream Sans2.004.
NOTO_BASE = f"https://raw.githubusercontent.com/notofonts/noto-cjk/{NOTO_COMMIT}/"
FONT_NAME = "NotoSansCJK-Regular.ttc"
FONT_URL = NOTO_BASE + "Sans/OTC/" + FONT_NAME
FONT_SHA256 = "b76b0433203017ca80401b2ee0dd69350349871c4b19d504c34dbdd80541690a"
LICENSE_URL = NOTO_BASE + "LICENSE"
LICENSE_SHA256 = "6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2"
FONT_LIMIT = 24 * 1024 * 1024
LICENSE_LIMIT = 64 * 1024
FONT_COPYRIGHT = "Copyright 2014-2021 Adobe (http://www.adobe.com/)."  # Pinned font's name ID 0.
FORMAT_VERSION = 1


class PreparationError(RuntimeError):
    pass


def digest_file(path: Path) -> str:
    checksum = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            checksum.update(chunk)
    return checksum.hexdigest()


def json_bytes(value: Any) -> bytes:
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2, allow_nan=False) + "\n").encode("utf-8")


def verified_download(url: str, destination: Path, checksum: str, limit: int, offline: bool) -> Path:
    if destination.is_file() and destination.stat().st_size <= limit and digest_file(destination) == checksum:
        return destination
    if offline:
        raise PreparationError(f"Offline font preparation needs a verified cached file: {destination}. Run once with network access or supply installed CJK fonts.")
    destination.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=destination.name + ".", suffix=".tmp", dir=destination.parent)
    try:
        request = urllib.request.Request(url, headers={"User-Agent": "FragileNotepad-font-preparation/1"})
        # Own the temporary descriptor before opening the network resource, so
        # connection failures close it through the same context manager.
        with os.fdopen(descriptor, "wb") as output, urllib.request.urlopen(request, timeout=45) as response:
            declared = response.headers.get("Content-Length")
            if declared and int(declared) > limit:
                raise PreparationError(f"Downloaded font resource exceeds the {limit}-byte limit: {url}")
            total = 0
            digest = hashlib.sha256()
            while chunk := response.read(1024 * 1024):
                total += len(chunk)
                if total > limit:
                    raise PreparationError(f"Downloaded font resource exceeds the {limit}-byte limit: {url}")
                digest.update(chunk)
                output.write(chunk)
        if digest.hexdigest() != checksum:
            raise PreparationError(f"Checksum verification failed for {url}; the cached font was not replaced.")
        os.replace(temporary, destination)
        return destination
    except (OSError, ValueError) as error:
        raise PreparationError(f"Cannot download verified font resource {url}: {error}") from error
    finally:
        Path(temporary).unlink(missing_ok=True)


def dependency_versions() -> dict[str, str]:
    import PIL
    import fontTools

    return {"Pillow": PIL.__version__, "fontTools": fontTools.__version__}


def dependency_python(cache: Path, offline: bool) -> Path | None:
    try:
        dependency_versions()
        return None
    except ImportError:
        pass
    if os.environ.get("FRAGILE_FONT_PREP_BOOTSTRAPPED") == "1":
        raise PreparationError("The isolated font-preparation environment is missing Pillow or fontTools.")
    requirement = SCRIPT_DIR / "requirements-font-profiles.txt"
    identity = digest_file(requirement)[:12]
    environment = cache / f"python-{sys.version_info.major}.{sys.version_info.minor}-{identity}"
    executable = environment / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
    if executable.is_file():
        usable = subprocess.run(
            [str(executable), "-c", "import PIL,fontTools"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30,
        ).returncode == 0
        if usable:
            return executable
    if offline:
        raise PreparationError("Offline font preparation requires Pillow and fontTools in the current Python or a populated cache venv. Run once online to prepare dependencies.")
    cache.mkdir(parents=True, exist_ok=True)
    try:
        subprocess.run([sys.executable, "-m", "venv", str(environment)], check=True)
        subprocess.run(
            [str(executable), "-m", "pip", "install", "--disable-pip-version-check", "--no-input", "-r", str(requirement)],
            check=True,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        raise PreparationError(f"Cannot bootstrap isolated font-preparation dependencies: {error}") from error
    return executable


def font_inventory(directories: list[Path]) -> list[dict[str, Any]]:
    paths = set()
    for directory in directories:
        if not directory.is_dir():
            continue
        try:
            paths.update(
                path.resolve() for path in directory.rglob("*")
                if path.is_file() and path.suffix.lower() in (".ttf", ".otf", ".ttc", ".otc")
            )
        except OSError as error:
            raise PreparationError(f"Cannot inventory font directory {directory}: {error}") from error
    inventory = []
    for path in sorted(paths, key=lambda value: str(value).casefold()):
        info = path.stat()
        inventory.append({"path": str(path), "size": info.st_size, "mtime_ns": info.st_mtime_ns})
    return inventory


def host_os() -> str:
    return "windows" if sys.platform == "win32" else "macos" if sys.platform == "darwin" else "linux"


def usable_family(faces: list[Face], family: str, region: int) -> bool:
    sample = generator.HAN_SAMPLE
    if region == 2:
        sample += "あいうえおカタカナ"
    elif region == 3:
        sample += generator.HANGUL_SAMPLE
    matches = [face for face in faces if family in face.aliases and face.normal]
    selected_weight = generator.query_weight((face.weight for face in matches), 400)
    return any(
        face.weight == selected_weight
        and (
            (face.weight == 400 and not face.bold)
            or (face.variable and face.weight_axis is not None and face.weight_axis[0] <= 400 <= face.weight_axis[1])
        )
        and face.covers(sample)
        and (region != 3 or generator.has_modern_hangul(face))
        for face in matches
    )


def selected_families(faces: list[Face], target_os: str) -> list[str | None]:
    for collection in catalog.coherent_collections():
        choices = [
            next((family for family in aliases if usable_family(faces, family, region)), None)
            for region, aliases in enumerate(collection)
        ]
        if all(choices):
            return choices
    return [
        next((family for family in aliases if usable_family(faces, family, region)), None)
        for region, aliases in enumerate(catalog.preferred_families(target_os))
    ]


def select_reference(faces: list[Face]) -> str | None:
    candidates = (
        "Microsoft JhengHei", "Noto Sans CJK TC", "Noto Sans Mono CJK TC", "Noto Sans TC",
        "Source Han Sans TC", "Source Han Sans TW",
    )
    for family in candidates:
        matches = [face for face in faces if family in face.aliases and face.normal]
        regular = generator.static_regular_face(matches)
        if regular is not None and regular.covers(generator.HAN_SAMPLE):
            return family
    return None


def rust_string(value: str) -> str:
    if "\0" in value:
        raise PreparationError("Font metadata or a cache path contains a NUL character")
    hashes = "#"
    while '"' + hashes in value:
        hashes += "#"
    return "r" + hashes + '"' + value + '"' + hashes


def rust_profiles(profiles: list[dict[str, Any]]) -> str:
    output = []
    seen = set()
    for profile in profiles:
        family = profile["family"]
        requested = profile["requested_weight"]
        actual = profile["face_weight"]
        scope = {"all": "All", "modern_hangul": "ModernHangul"}.get(profile["scope"])
        if (
            not isinstance(family, str) or family in seen or type(requested) is not int
            or requested not in range(100, 901, 100) or type(actual) is not int
            or not 1 <= actual <= 1000 or scope is None
        ):
            raise PreparationError("Cached or generated profile has invalid fields or duplicate families")
        seen.add(family)
        output.append(f"    WeightRule {{ family: {rust_string(family)}, requested_weight: {requested}, face_weight: {actual}, scope: Scope::{scope} }},")
    return "&[\n" + "\n".join(output) + "\n]\n" if output else "&[]\n"


def rust_assets(paths: list[Path]) -> str:
    if not paths:
        return "&[]\n"
    expressions = [f"    include_bytes!({rust_string(path.resolve().as_posix())}) as &[u8]," for path in paths]
    return "&[\n" + "\n".join(expressions) + "\n]\n"


def rust_families(target_os: str) -> str:
    rows = ["    &[" + ", ".join(rust_string(name) for name in families) + "]," for families in catalog.preferred_families(target_os)]
    return "&[\n" + "\n".join(rows) + "\n]\n"


def rust_collections() -> str:
    rows = []
    for collection in catalog.coherent_collections():
        regions = ["&[" + ", ".join(rust_string(name) for name in aliases) + "]" for aliases in collection]
        rows.append("    [" + ", ".join(regions) + "],")
    return "&[\n" + "\n".join(rows) + "\n]\n"


def cache_identity(args: argparse.Namespace, inventory: list[dict[str, Any]], dependencies: dict[str, str]) -> tuple[str, dict[str, Any]]:
    inputs = {
        "version": FORMAT_VERSION,
        "target_os": args.target_os,
        "host_os": host_os(),
        "font_directories": [str(path.resolve()) for path in args.font_dir],
        "font_inventory": inventory,
        "dependencies": dependencies,
        "scripts": {name: digest_file(SCRIPT_DIR / name) for name in (
            "prepare_font_profiles.py", "generate_font_profiles.py", "font_families.py", "requirements-font-profiles.txt",
        )},
        "calibration": generator.CALIBRATION_DEFAULTS,
        "download": {"commit": NOTO_COMMIT, "font_sha256": FONT_SHA256, "license_sha256": LICENSE_SHA256},
    }
    return hashlib.sha256(json_bytes(inputs)).hexdigest(), inputs


def cached_result(path: Path, fingerprint: str) -> dict[str, Any] | None:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
        if value["fingerprint"] != fingerprint:
            return None
        rust_profiles(value["profiles"])
        for file in value["verified_files"]:
            source = Path(file["path"])
            if not source.is_file() or digest_file(source) != file["sha256"]:
                return None
        return value
    except (OSError, ValueError, KeyError, TypeError, PreparationError):
        return None


def emit_outputs(out_dir: Path, result: dict[str, Any], directories: list[Path], inventory: list[dict[str, Any]]) -> None:
    assets = [Path(path) for path in result["embedded_assets"]]
    atomic_write(out_dir / "font_profiles.rs", rust_profiles(result["profiles"]).encode("utf-8"))
    atomic_write(out_dir / "font_assets.rs", rust_assets(assets).encode("utf-8"))
    atomic_write(out_dir / "font_families.rs", rust_families(result["target_os"]).encode("utf-8"))
    atomic_write(out_dir / "font_collections.rs", rust_collections().encode("utf-8"))
    notices = ""
    if assets:
        license_text = Path(result["license_path"]).read_text(encoding="utf-8")
        notices = f"Noto Sans CJK Regular (Sans2.004), bundled under the SIL Open Font License 1.1\n{FONT_COPYRIGHT}\nSource: {FONT_URL}\nSHA-256: {FONT_SHA256}\n\n" + license_text
        atomic_write(out_dir / "licenses/NotoSansCJK/OFL.txt", license_text.encode("utf-8"))
    atomic_write(out_dir / "FONT-NOTICES.txt", notices.encode("utf-8"))
    tracked = set(str(path.resolve()) for path in directories if path.is_dir())
    tracked.update(file["path"] for file in inventory)
    tracked.update(file["path"] for file in result["verified_files"] if file["kind"] in ("font", "license"))
    atomic_write(out_dir / "rerun-paths.txt", ("\n".join(sorted(tracked)) + "\n").encode("utf-8"))
    atomic_write(out_dir / "font_profiles_manifest.json", json_bytes(result))


def prepare(args: argparse.Namespace) -> dict[str, Any]:
    directories = [path.resolve() for path in args.font_dir]
    inventory = font_inventory(directories)
    fingerprint, inputs = cache_identity(args, inventory, dependency_versions())
    cache = args.cache_dir.resolve()
    entry = cache / "measurements" / fingerprint
    manifest = entry / "manifest.json"
    if result := cached_result(manifest, fingerprint):
        emit_outputs(args.out_dir, result, directories, inventory)
        print(f"Font preparation reused verified {args.target_os} cache ({len(result['profiles'])} profiles).")
        return result
    installed, warnings = generator.scan_faces(directories)
    selected = selected_families(installed, args.target_os)
    reference = select_reference(installed)
    embed = not all(selected) or host_os() != args.target_os
    download = embed or reference is None
    calibration_directories = directories.copy()
    calibration_faces = installed
    verified_files = []
    font_path = None
    license_path = None
    if download:
        download_directory = cache / "downloads"
        font_path = verified_download(FONT_URL, download_directory / FONT_NAME, FONT_SHA256, FONT_LIMIT, args.offline)
        license_path = verified_download(LICENSE_URL, download_directory / "OFL.txt", LICENSE_SHA256, LICENSE_LIMIT, args.offline)
        verified_files.extend([
            {"kind": "font", "path": str(font_path), "sha256": FONT_SHA256},
            {"kind": "license", "path": str(license_path), "sha256": LICENSE_SHA256},
        ])
        calibration_directories.append(download_directory)
        downloaded, downloaded_warnings = generator.scan_faces([download_directory])
        warnings.extend(downloaded_warnings)
        downloaded_aliases = {alias for face in downloaded for alias in face.aliases}
        # A partial or older installed Noto collection may duplicate this pinned
        # reference. Calibrate its canonical downloaded faces once, keeping all
        # other installed families available for native-route measurements.
        calibration_faces = downloaded + [
            face for face in installed if downloaded_aliases.isdisjoint(face.aliases)
        ]
        if embed:
            # The embedded coherent collection wins the same priority as at runtime.
            selected = selected_families(calibration_faces, args.target_os)
        if reference is None:
            reference = select_reference(downloaded)
    if not all(selected) or reference is None:
        raise PreparationError("Installed and verified downloaded fonts cannot provide every regional family and a static reference.")
    entry.mkdir(parents=True, exist_ok=True)
    candidate_path = entry / "generated-cjk-profiles.json"
    report_path = entry / "generated-cjk-profiles.report.json"
    arguments = ["--reference-family", reference, "--output", str(candidate_path), "--report", str(report_path)]
    for directory in calibration_directories:
        arguments += ["--font-dir", str(directory)]
    for family in dict.fromkeys(selected):
        arguments += ["--family", family]
    if generator.main(arguments, scanned_faces=(calibration_faces, warnings)) != 0:
        raise PreparationError("Optical-profile generation failed; no build outputs were adopted.")
    try:
        profiles = json.loads(candidate_path.read_text(encoding="utf-8"))["profiles"]
    except (OSError, ValueError, KeyError) as error:
        raise PreparationError(f"Generator produced an unreadable profile result: {error}") from error
    rust_profiles(profiles)
    verified_files.extend([
        {"kind": "candidates", "path": str(candidate_path), "sha256": digest_file(candidate_path)},
        {"kind": "report", "path": str(report_path), "sha256": digest_file(report_path)},
    ])
    result = {
        "version": FORMAT_VERSION,
        "fingerprint": fingerprint,
        "target_os": args.target_os,
        "inputs": inputs,
        "selected_families": selected,
        "reference_family": reference,
        "profiles": profiles,
        "embedded_assets": [str(font_path)] if embed else [],
        "license_path": str(license_path) if license_path else None,
        "verified_files": verified_files,
        "warnings": warnings,
        "cache_manifest": str(manifest),
    }
    atomic_write(manifest, json_bytes(result))
    emit_outputs(args.out_dir, result, directories, inventory)
    return result


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out-dir", type=Path, required=True, help="Cargo OUT_DIR or an explicit prepared-output directory")
    parser.add_argument("--target-os", choices=catalog.TARGET_OSES, required=True)
    parser.add_argument("--cache-dir", type=Path, default=REPOSITORY / "target/font-profiles")
    parser.add_argument("--font-dir", type=Path, action="append", help="repeat to replace default system font directories")
    args = parser.parse_args(argv)
    if args.font_dir is None:
        args.font_dir = generator.default_font_dirs()
    args.offline = os.environ.get("FRAGILE_FONT_OFFLINE") == "1"
    return args


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        executable = dependency_python(args.cache_dir.resolve(), args.offline)
        if executable:
            environment = os.environ.copy()
            environment["FRAGILE_FONT_PREP_BOOTSTRAPPED"] = "1"
            return subprocess.run([str(executable), str(Path(__file__).resolve()), *(argv if argv is not None else sys.argv[1:])], env=environment).returncode
        prepare(args)
        return 0
    except (OSError, subprocess.SubprocessError, PreparationError) as error:
        print(f"Font preparation failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
