use std::path::{Path, PathBuf};
use wattwarden_core::{GpuController, Result, WattWardenError};
use crate::sysfs::{read_sysfs_u32, write_sysfs_string};

#[derive(Debug, Clone)]
pub struct LinuxGpu {
    card_path: PathBuf,
}

impl LinuxGpu {
    pub fn new() -> Result<Self> {
        for card in &["/sys/class/drm/card1", "/sys/class/drm/card0"] {
            let p = Path::new(card);
            if p.join("gt_max_freq_mhz").exists() {
                return Ok(Self {
                    card_path: p.to_path_buf(),
                });
            }
        }
        Err(WattWardenError::InterfaceNotFound("No integrated or discrete GPU frequency sysfs node found".into()))
    }
}

impl GpuController for LinuxGpu {
    fn gpu_bounds(&self) -> Result<(u32, u32)> {
        let min_p = self.card_path.join("gt_RPn_freq_mhz");
        let min_fallback = self.card_path.join("gt_min_freq_mhz");
        let min_mhz = read_sysfs_u32(&min_p)
            .or_else(|_| read_sysfs_u32(&min_fallback))
            .unwrap_or(300);

        let max_p = self.card_path.join("gt_RP0_freq_mhz");
        let max_fallback = self.card_path.join("gt_max_freq_mhz");
        let max_mhz = read_sysfs_u32(&max_p)
            .or_else(|_| read_sysfs_u32(&max_fallback))
            .unwrap_or(1100);

        Ok((min_mhz, max_mhz))
    }

    fn gpu_freq(&self) -> Result<u32> {
        let max_p = self.card_path.join("gt_max_freq_mhz");
        read_sysfs_u32(&max_p)
    }

    fn set_gpu_freq(&self, mhz: u32) -> Result<()> {
        let (min_b, max_b) = self.gpu_bounds()?;
        let target = mhz.clamp(min_b, max_b);

        let min_p = self.card_path.join("gt_min_freq_mhz");
        let max_p = self.card_path.join("gt_max_freq_mhz");

        let _ = write_sysfs_string(&min_p, &min_b.to_string());
        write_sysfs_string(&max_p, &target.to_string())
    }
}
