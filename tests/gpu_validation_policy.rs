#[path = "support/gpu_validation.rs"]
mod gpu_validation;

use std::process::{Command, Output};

// Run policy failures in a child harness to avoid mutating this test process's
// environment while other tests might be using it.
fn run_without_adapter(setting: Option<&str>, ci: bool) -> Output {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "unavailable_adapter_child", "--nocapture"])
        .env("FRAGILE_GPU_TEST_CHILD", "1")
        .env_remove("FRAGILE_REQUIRE_GPU_TESTS")
        .env_remove("CI");
    if let Some(setting) = setting {
        command.env("FRAGILE_REQUIRE_GPU_TESTS", setting);
    }
    if ci {
        command.env("CI", "true");
    }
    command.output().unwrap()
}

#[test]
fn unavailable_adapter_child() {
    if std::env::var_os("FRAGILE_GPU_TEST_CHILD").is_some() {
        gpu_validation::unavailable("test adapter unavailable");
    }
}

#[test]
fn mandatory_gpu_checks_fail_without_an_adapter() {
    let output = run_without_adapter(Some("1"), false);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("GPU validation is required"));
}

#[test]
fn ci_requires_an_adapter_by_default() {
    assert!(!run_without_adapter(None, true).status.success());
}

#[test]
fn local_checks_can_skip_an_unavailable_adapter() {
    let output = run_without_adapter(None, false);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Skipping GPU validation"));
}

#[test]
fn explicit_optional_checks_can_skip_in_ci() {
    assert!(run_without_adapter(Some("0"), true).status.success());
}

#[test]
fn invalid_requirement_cannot_silently_skip() {
    let output = run_without_adapter(Some("yes"), false);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("must be 0 or 1"));
}
