use crate::sysfs::{read_sysfs_u32, write_sysfs_string};
use std::path::{Path, PathBuf};
use wattwarden_core::{GpuController, Result, WattWardenError};

#[derive(Debug, Clone)]
pub struct LinuxGpu {
    card_path: PathBuf,
}

impl LinuxGpu {
    pub fn new() -> Result<Self> {
        let drm_dir = Path::new("/sys/class/drm");
        if let Ok(entries) = std::fs::read_dir(drm_dir) {
            let mut cards = Vec::new();
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("card") && !name.contains('-') {
                    cards.push(entry.path());
                }
            }
            cards.sort();
            cards.reverse();
            for p in cards {
                if p.join("gt_max_freq_mhz").exists() {
                    return Ok(Self { card_path: p });
                }
            }
        }
        Err(WattWardenError::InterfaceNotFound(
            "No controllable GPU frequency interface discovered in /sys/class/drm".into(),
        ))
    }

    pub fn with_card_path(card_path: PathBuf) -> Self {
        Self { card_path }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_mock_gpu_controller() {
        let tmp_dir = std::env::temp_dir().join(format!("ww_gpu_test_{}", std::process::id()));
        let card_dir = tmp_dir.join("card0");
        let _ = fs::create_dir_all(&card_dir);

        fs::write(card_dir.join("gt_RPn_freq_mhz"), "350\n").unwrap();
        fs::write(card_dir.join("gt_RP0_freq_mhz"), "1200\n").unwrap();
        fs::write(card_dir.join("gt_min_freq_mhz"), "350\n").unwrap();
        fs::write(card_dir.join("gt_max_freq_mhz"), "1000\n").unwrap();

        let gpu = LinuxGpu::with_card_path(card_dir.clone());
        let (min_b, max_b) = gpu.gpu_bounds().unwrap();
        assert_eq!(min_b, 350);
        assert_eq!(max_b, 1200);

        assert_eq!(gpu.gpu_freq().unwrap(), 1000);

        gpu.set_gpu_freq(800).unwrap();
        assert_eq!(gpu.gpu_freq().unwrap(), 800);

        let _ = fs::remove_dir_all(tmp_dir);
    }
}
