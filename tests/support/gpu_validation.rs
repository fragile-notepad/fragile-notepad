//! Shared policy for application tests that need a GPU adapter.

const REQUIRE_GPU: &str = "FRAGILE_REQUIRE_GPU_TESTS";

/// Fail mandatory graphics validation instead of reporting an untested pass.
/// Device creation, renderer initialization, rendering, and readback failures
/// must fail unconditionally; call this only when an adapter is unavailable.
pub fn unavailable(reason: &str) {
    let required = match std::env::var(REQUIRE_GPU) {
        Ok(value) => match value.as_str() {
            "1" => true,
            "0" => false,
            _ => panic!("{REQUIRE_GPU} must be 0 or 1, got {value:?}"),
        },
        Err(std::env::VarError::NotPresent) => std::env::var_os("CI").is_some(),
        Err(error) => panic!("cannot read {REQUIRE_GPU}: {error}"),
    };
    assert!(
        !required,
        "GPU validation is required ({REQUIRE_GPU}=1 or CI is set): {reason}"
    );
    eprintln!("Skipping GPU validation: {reason}; set {REQUIRE_GPU}=1 to require it");
}
