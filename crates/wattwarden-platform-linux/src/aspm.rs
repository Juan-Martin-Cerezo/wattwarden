use wattwarden_core::{AspmController, Result, WattWardenError};
use crate::sysfs::{read_sysfs_string, write_sysfs_string};

const ASPM_POLICY_PATH: &str = "/sys/module/pcie_aspm/parameters/policy";

#[derive(Debug, Clone, Default)]
pub struct LinuxAspm;

impl LinuxAspm {
    pub fn new() -> Result<Self> {
        if std::path::Path::new(ASPM_POLICY_PATH).exists() {
            Ok(Self)
        } else {
            Err(WattWardenError::InterfaceNotFound("PCIe ASPM sysfs parameter not available".into()))
        }
    }
}

impl AspmController for LinuxAspm {
    fn aspm_policy(&self) -> Result<String> {
        let content = read_sysfs_string(ASPM_POLICY_PATH)?;
        for word in content.split_whitespace() {
            if word.starts_with('[') && word.ends_with(']') {
                return Ok(word[1..word.len() - 1].to_string());
            }
        }
        Ok(content)
    }

    fn set_aspm_policy(&self, policy: &str) -> Result<()> {
        write_sysfs_string(ASPM_POLICY_PATH, policy)
    }
}
