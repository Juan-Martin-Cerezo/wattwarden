use crate::sysfs::{read_sysfs_string, read_sysfs_u64, write_sysfs_u64};
use std::path::PathBuf;
use wattwarden_core::{RaplController, Result, WattWardenError};

pub struct LinuxRapl {
    package_path: PathBuf,
}

impl LinuxRapl {
    pub fn new() -> Result<Self> {
        let package_path = PathBuf::from("/sys/class/powercap/intel-rapl:0");
        if !package_path.exists() {
            return Err(WattWardenError::InterfaceNotFound(
                "Intel/AMD RAPL interface not present at /sys/class/powercap/intel-rapl:0".into(),
            ));
        }
        Ok(Self { package_path })
    }

    fn find_constraint_path(&self, constraint_name: &str) -> Option<PathBuf> {
        for i in 0..5 {
            let name_path = self.package_path.join(format!("constraint_{}_name", i));
            if let Ok(name) = read_sysfs_string(&name_path) {
                if name.trim().eq_ignore_ascii_case(constraint_name) {
                    return Some(
                        self.package_path
                            .join(format!("constraint_{}_power_limit_uw", i)),
                    );
                }
            }
        }
        None
    }
}

impl RaplController for LinuxRapl {
    fn rapl_bounds(&self) -> Result<(u32, u32)> {
        let min_uw =
            read_sysfs_u64(self.package_path.join("min_power_range_uw")).unwrap_or(5_000_000);
        let max_uw =
            read_sysfs_u64(self.package_path.join("max_power_range_uw")).unwrap_or(115_000_000);
        Ok(((min_uw / 1_000_000) as u32, (max_uw / 1_000_000) as u32))
    }

    fn pl1_watts(&self) -> Result<u32> {
        let path = self.find_constraint_path("long_term").ok_or_else(|| {
            WattWardenError::InterfaceNotFound("PL1 long_term constraint not found".into())
        })?;
        let uw = read_sysfs_u64(path)?;
        Ok((uw / 1_000_000) as u32)
    }

    fn set_pl1_watts(&self, watts: u32) -> Result<()> {
        let (min_w, max_w) = self.rapl_bounds()?;
        let clamped = watts.clamp(min_w, max_w);
        let path = self.find_constraint_path("long_term").ok_or_else(|| {
            WattWardenError::InterfaceNotFound("PL1 long_term constraint not found".into())
        })?;
        write_sysfs_u64(path, (clamped as u64) * 1_000_000)
    }

    fn pl2_watts(&self) -> Result<u32> {
        let path = self.find_constraint_path("short_term").ok_or_else(|| {
            WattWardenError::InterfaceNotFound("PL2 short_term constraint not found".into())
        })?;
        let uw = read_sysfs_u64(path)?;
        Ok((uw / 1_000_000) as u32)
    }

    fn set_pl2_watts(&self, watts: u32) -> Result<()> {
        let (min_w, max_w) = self.rapl_bounds()?;
        let clamped = watts.clamp(min_w, max_w);
        let path = self.find_constraint_path("short_term").ok_or_else(|| {
            WattWardenError::InterfaceNotFound("PL2 short_term constraint not found".into())
        })?;
        write_sysfs_u64(path, (clamped as u64) * 1_000_000)
    }
}
