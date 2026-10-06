fn main() {
    prepare_font_profiles();
    println!("cargo:rerun-if-changed=target/app-icons/app.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("target/app-icons/app.ico")
            .compile()
            .expect("compile Windows icon resource; run scripts/generate_icon_assets.ps1 first");
    }
}

fn prepare_font_profiles() {
    use std::path::PathBuf;
    use std::process::Command;

    for script in [
        "scripts/prepare_font_profiles.py",
        "scripts/generate_font_profiles.py",
        "scripts/font_families.py",
        "scripts/requirements-font-profiles.txt",
    ] {
        println!("cargo:rerun-if-changed={script}");
    }
    for variable in [
        "FRAGILE_FONT_PYTHON",
        "FRAGILE_FONT_CACHE",
        "FRAGILE_FONT_OFFLINE",
    ] {
        println!("cargo:rerun-if-env-changed={variable}");
    }

    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("package directory"));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("build output directory"));
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").expect("target platform");
    let python = std::env::var_os("FRAGILE_FONT_PYTHON")
        .unwrap_or_else(|| if cfg!(windows) { "python" } else { "python3" }.into());
    let cache = std::env::var_os("FRAGILE_FONT_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/font-profiles"));
    let output = Command::new(&python)
        .arg(root.join("scripts/prepare_font_profiles.py"))
        .arg("--out-dir")
        .arg(&out)
        .arg("--target-os")
        .arg(&target_os)
        .arg("--cache-dir")
        .arg(cache)
        .current_dir(&root)
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "run font preparation with {python:?}: {error}; install Python 3 or set FRAGILE_FONT_PYTHON"
            )
        });
    if !output.status.success() {
        panic!(
            "font preparation failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Follow the installed font inventory as well as the generation scripts.
    // Cached profiles are reused until one of those inputs changes.
    let paths = std::fs::read_to_string(out.join("rerun-paths.txt"))
        .expect("font preparation rerun inputs");
    for path in paths.lines().filter(|path| !path.is_empty()) {
        println!("cargo:rerun-if-changed={path}");
    }

    // Keep downloaded fonts' notices beside the build artifact for distribution.
    if let Some(artifact_dir) = out.ancestors().nth(3) {
        std::fs::copy(
            out.join("FONT-NOTICES.txt"),
            artifact_dir.join("FONT-NOTICES.txt"),
        )
        .expect("copy generated font notices");
    }
}
