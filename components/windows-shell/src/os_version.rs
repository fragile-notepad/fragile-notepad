use ::windows::{
    Wdk::System::SystemServices::RtlGetVersion, Win32::System::SystemInformation::OSVERSIONINFOW,
    core::Result,
};

/// The Explorer menu integration available on the running Windows generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MenuGeneration {
    Legacy,
    Modern,
}

/// Read the actual OS version, independently of the host executable's manifest.
/// Windows 11 retains NT version 10.0, so its build number is the discriminator.
pub(crate) fn current_menu_generation() -> Result<MenuGeneration> {
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: `version` has the size and layout required by RtlGetVersion and
    // remains writable for the duration of the call.
    unsafe { RtlGetVersion(&mut version).ok()? };
    Ok(classify_version(
        version.dwMajorVersion,
        version.dwBuildNumber,
    ))
}

const fn classify_version(major: u32, build: u32) -> MenuGeneration {
    const FIRST_WINDOWS_11_BUILD: u32 = 22_000;
    if major > 10 || (major == 10 && build >= FIRST_WINDOWS_11_BUILD) {
        MenuGeneration::Modern
    } else {
        MenuGeneration::Legacy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_11_starts_at_build_22000() {
        assert_eq!(classify_version(10, 21_999), MenuGeneration::Legacy);
        assert_eq!(classify_version(10, 22_000), MenuGeneration::Modern);
    }

    #[test]
    fn windows_10_uses_legacy_integration() {
        for build in [10_240, 19_041, 19_045] {
            assert_eq!(classify_version(10, build), MenuGeneration::Legacy);
        }
    }

    #[test]
    fn windows_11_releases_use_modern_integration() {
        for build in [22_000, 22_621, 22_631, 26_100] {
            assert_eq!(classify_version(10, build), MenuGeneration::Modern);
        }
    }

    #[test]
    fn older_major_versions_cannot_be_classified_by_build_alone() {
        assert_eq!(classify_version(6, 9_200), MenuGeneration::Legacy);
        assert_eq!(classify_version(6, 9_600), MenuGeneration::Legacy);
        assert_eq!(classify_version(6, 26_100), MenuGeneration::Legacy);
    }

    #[test]
    fn future_major_versions_use_modern_integration() {
        assert_eq!(classify_version(11, 0), MenuGeneration::Modern);
    }

    #[test]
    fn running_os_version_can_be_queried() {
        current_menu_generation().expect("RtlGetVersion must return the running OS version");
    }
}
