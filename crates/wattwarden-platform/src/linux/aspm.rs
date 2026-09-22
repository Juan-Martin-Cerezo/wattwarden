//! PCIe Active State Power Management (ASPM) policy.
//!
//! Faithful transcription of `hal/backend_linux.go`:
//!
//! * `GetASPM()` reads `/sys/module/pcie_aspm/parameters/policy` and returns the
//!   token wrapped in `[...]` (the kernel marks the active policy that way). When
//!   there are no brackets the raw content is returned, and a missing file yields
//!   `""` exactly like Go's `readSys`.
//! * `SetASPM(p)` writes the policy verbatim.
//!
//! The path is resolved through [`SysfsRoot`] so the whole probe is relocatable.

use crate::linux::sysfs::SysfsRoot;
use wattwarden_core::{AspmController, Result, WattWardenError};

const ASPM_POLICY_PATH: &str = "sys/module/pcie_aspm/parameters/policy";

#[derive(Debug, Clone)]
pub struct LinuxAspm {
    root: SysfsRoot,
}

impl LinuxAspm {
    /// Uses the root from `WATTWARDEN_SYSFS_ROOT` (default `/`).
    pub fn new() -> Result<Self> {
        Self::with_root(SysfsRoot::from_env())
    }

    pub fn with_root(root: SysfsRoot) -> Result<Self> {
        if !root.exists(ASPM_POLICY_PATH) {
            return Err(WattWardenError::InterfaceNotFound(
                "PCIe ASPM sysfs parameter not available".into(),
            ));
        }
        Ok(Self { root })
    }
}

impl Default for LinuxAspm {
    fn default() -> Self {
        Self::new().expect("ASPM parameter unavailable")
    }
}

/// Go `GetASPM`: first whitespace token wrapped in brackets, else the raw string.
pub fn parse_aspm_policy(content: &str) -> String {
    for word in content.split_whitespace() {
        if word.starts_with('[') && word.ends_with(']') && word.len() >= 2 {
            return word[1..word.len() - 1].to_string();
        }
    }
    content.trim().to_string()
}

impl AspmController for LinuxAspm {
    fn aspm_policy(&self) -> Result<String> {
        // Go `readSys` never errors; a missing file simply reads as "".
        Ok(parse_aspm_policy(&self.root.read(ASPM_POLICY_PATH)))
    }

    fn set_aspm_policy(&self, policy: &str) -> Result<()> {
        self.root.write_best_effort(ASPM_POLICY_PATH, policy);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root(tag: &str, content: &str) -> SysfsRoot {
        let dir = std::env::temp_dir().join(format!("ww_aspm_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join(ASPM_POLICY_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
        SysfsRoot::new(dir)
    }

    #[test]
    fn test_parse_aspm_policy() {
        assert_eq!(
            parse_aspm_policy("default [powersave] performance"),
            "powersave"
        );
        assert_eq!(
            parse_aspm_policy("[default] powersave performance"),
            "default"
        );
        assert_eq!(parse_aspm_policy("performance"), "performance");
        assert_eq!(parse_aspm_policy("[] powersave"), "");
    }

    #[test]
    fn get_and_set_respect_the_root_override() {
        let root = root("rw", "default [powersave] performance\n");
        let aspm = LinuxAspm::with_root(root.clone()).unwrap();
        assert_eq!(aspm.aspm_policy().unwrap(), "powersave");

        aspm.set_aspm_policy("performance").unwrap();
        assert_eq!(root.read(ASPM_POLICY_PATH), "performance");
        assert_eq!(aspm.aspm_policy().unwrap(), "performance");
    }

    #[test]
    fn missing_parameter_is_reported() {
        let dir = std::env::temp_dir().join(format!("ww_aspm_none_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(LinuxAspm::with_root(SysfsRoot::new(dir)).is_err());
    }
}
