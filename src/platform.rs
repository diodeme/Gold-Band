use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DesktopOperatingSystem {
    Windows,
    Macos,
    Linux,
}

impl DesktopOperatingSystem {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Linux => "linux",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DesktopArchitecture {
    #[serde(rename = "x86_64")]
    X86_64,
    #[serde(rename = "aarch64")]
    Aarch64,
}

impl DesktopArchitecture {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPlatform {
    pub os: DesktopOperatingSystem,
    pub arch: DesktopArchitecture,
}

impl DesktopPlatform {
    pub const fn new(os: DesktopOperatingSystem, arch: DesktopArchitecture) -> Self {
        Self { os, arch }
    }

    pub fn current() -> Self {
        normalize_desktop_platform(std::env::consts::OS, std::env::consts::ARCH)
            .expect("Gold Band desktop platform is supported")
    }
}

pub fn normalize_desktop_platform(os: &str, arch: &str) -> Result<DesktopPlatform> {
    let os = match os {
        "windows" => DesktopOperatingSystem::Windows,
        "macos" => DesktopOperatingSystem::Macos,
        "linux" => DesktopOperatingSystem::Linux,
        value => bail!("unsupported desktop OS: {value}"),
    };
    let arch = match arch {
        "x86_64" => DesktopArchitecture::X86_64,
        "aarch64" => DesktopArchitecture::Aarch64,
        value => bail!("unsupported desktop CPU architecture: {value}"),
    };
    Ok(DesktopPlatform::new(os, arch))
}

pub fn platform_excluded(excluded: &[DesktopPlatform], platform: DesktopPlatform) -> bool {
    excluded.contains(&platform)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_rust_platform_names_to_catalog_wire_values() {
        assert_eq!(
            normalize_desktop_platform("macos", "aarch64").unwrap(),
            DesktopPlatform::new(DesktopOperatingSystem::Macos, DesktopArchitecture::Aarch64)
        );
        assert_eq!(
            normalize_desktop_platform("windows", "x86_64").unwrap(),
            DesktopPlatform::new(DesktopOperatingSystem::Windows, DesktopArchitecture::X86_64)
        );
        assert!(normalize_desktop_platform("win32", "arm64").is_err());
    }

    #[test]
    fn platform_exclusion_matches_only_complete_os_and_arch_pairs() {
        let excluded = [DesktopPlatform::new(
            DesktopOperatingSystem::Windows,
            DesktopArchitecture::Aarch64,
        )];
        assert!(platform_excluded(
            &excluded,
            DesktopPlatform::new(
                DesktopOperatingSystem::Windows,
                DesktopArchitecture::Aarch64
            )
        ));
        assert!(!platform_excluded(
            &excluded,
            DesktopPlatform::new(DesktopOperatingSystem::Windows, DesktopArchitecture::X86_64)
        ));
        assert!(!platform_excluded(
            &excluded,
            DesktopPlatform::new(DesktopOperatingSystem::Linux, DesktopArchitecture::Aarch64)
        ));
    }
}
