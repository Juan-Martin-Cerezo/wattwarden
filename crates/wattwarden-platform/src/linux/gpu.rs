//! Intel/AMD integrated GPU frequency control via the DRM `gt_*` sysfs knobs.
//!
//! Faithful transcription of `hal/backend_linux.go`:
//!
//! * `getGPUPath()` -> `/sys/class/drm/card1` when `gt_max_freq_mhz` exists there,
//!   otherwise `/sys/class/drm/card0`, otherwise "no support".
//! * `GetGPUBounds()` -> no card means `(300, 1100)`; otherwise
//!   min = `gt_RPn_freq_mhz` else `gt_min_freq_mhz`, max = `gt_RP0_freq_mhz` else
//!   `gt_max_freq_mhz`, each falling back to `300`/`1100` on parse failure. This is
//!   the **display** path: a written value (`gt_min`/`gt_max`) may appear in it, which
//!   is fine for a UI fallback but is never allowed to gate a write.
//! * `GetGPUFreq()` -> 0 when there is no card or the file is unparsable.
//! * `SetGPUFreq(mhz)` -> no card is a no-op; otherwise clamp to bounds and write
//!   `gt_min_freq_mhz = min` **first**, then `gt_max_freq_mhz = mhz` (Go warns that
//!   writing the max below the current min is rejected by the kernel, hence order).

use crate::linux::sysfs::SysfsRoot;
use std::path::PathBuf;
use tracing::debug;
use wattwarden_core::{GpuController, Result, WattWardenError};

/// Relative base of the DRM class directory.
const DRM_BASE: &str = "sys/class/drm";

/// Go fallbacks for `GetGPUBounds`.
pub const FALLBACK_MIN_MHZ: u32 = 300;
pub const FALLBACK_MAX_MHZ: u32 = 1100;

#[derive(Debug, Clone)]
pub struct LinuxGpu {
    root: SysfsRoot,
    /// Absolute path of the discovered card directory (`None` = unsupported).
    card: Option<PathBuf>,
}

impl LinuxGpu {
    /// Uses the root from `WATTWARDEN_SYSFS_ROOT` (default `/`).
    pub fn new() -> Result<Self> {
        Self::with_root(SysfsRoot::from_env())
    }

    pub fn with_root(root: SysfsRoot) -> Result<Self> {
        let card = Self::discover(&root);
        if card.is_none() {
            return Err(WattWardenError::InterfaceNotFound(
                "No controllable GPU frequency interface discovered in /sys/class/drm".into(),
            ));
        }
        Ok(Self { root, card })
    }

    /// Compatibility constructor: point the controller at an explicit absolute card
    /// path (used by tests and by callers that already resolved the node).
    pub fn with_card_path(card_path: PathBuf) -> Self {
        Self {
            root: SysfsRoot::new("/"),
            card: Some(card_path),
        }
    }

    /// Go `getGPUPath()`: `card1` first, then `card0`, else nothing.
    fn discover(root: &SysfsRoot) -> Option<PathBuf> {
        ["card1", "card0"].iter().find_map(|card| {
            let rel = format!("{DRM_BASE}/{card}");
            root.exists(&format!("{rel}/gt_max_freq_mhz"))
                .then(|| root.path(&rel))
        })
    }

    /// True when a card exposing `gt_max_freq_mhz` was found.
    pub fn is_supported(&self) -> bool {
        self.card.is_some()
    }

    fn card_read(&self, leaf: &str) -> Option<u32> {
        let card = self.card.as_ref()?;
        let raw = self.root.read_path(&card.join(leaf));
        if raw.is_empty() {
            return None;
        }
        raw.parse::<u32>().ok()
    }

    fn card_write(&self, leaf: &str, value: u32) {
        if let Some(card) = self.card.as_ref() {
            self.root
                .write_best_effort_path(&card.join(leaf), &value.to_string());
        }
    }
}

impl GpuController for LinuxGpu {
    /// `(min, max)` MHz discovered from the card's **immutable** hardware info:
    /// `gt_RPn_freq_mhz` and `gt_RP0_freq_mhz`.
    ///
    /// `gt_min_freq_mhz`/`gt_max_freq_mhz` are deliberately **not** used here: they are
    /// the nodes [`GpuController::set_gpu_freq`] writes, so reading them back to
    /// discover the range would let it ratchet with every write (exactly the CPU bug,
    /// in the display path). They are for writing only.
    ///
    /// `None` when there is no card, either immutable bound is missing, or the range is
    /// degenerate (`min == max`): callers must then **not write**.
    /// [`GpuController::gpu_bounds`] keeps the legacy `300/1100` fallback for display.
    fn discovered_gpu_bounds(&self) -> Option<(u32, u32)> {
        self.card.as_ref()?;
        let min = self.card_read("gt_RPn_freq_mhz")?;
        let max = self.card_read("gt_RP0_freq_mhz")?;
        if max <= min {
            debug!("GPU: degenerate gt_RPn/gt_RP0 range ({min}..{max} MHz); freq undiscoverable");
            return None;
        }
        Some((min, max))
    }

    fn gpu_bounds(&self) -> Result<(u32, u32)> {
        if self.card.is_none() {
            return Ok((FALLBACK_MIN_MHZ, FALLBACK_MAX_MHZ));
        }

        let min = self
            .card_read("gt_RPn_freq_mhz")
            .or_else(|| self.card_read("gt_min_freq_mhz"))
            .unwrap_or(FALLBACK_MIN_MHZ);

        let max = self
            .card_read("gt_RP0_freq_mhz")
            .or_else(|| self.card_read("gt_max_freq_mhz"))
            .unwrap_or(FALLBACK_MAX_MHZ);

        Ok((min, max))
    }

    fn gpu_freq(&self) -> Result<u32> {
        // Go: missing card or unparsable value both yield 0.
        Ok(self.card_read("gt_max_freq_mhz").unwrap_or(0))
    }

    fn set_gpu_freq(&self, mhz: u32) -> Result<()> {
        // No card, a missing bound, or a degenerate range (min == max) -> nothing is
        // written. The legacy 300/1100 fallback is never used to pick a written value.
        let Some((min, max)) = self.discovered_gpu_bounds() else {
            debug!("GPU: no usable frequency range discovered; not writing");
            return Ok(());
        };
        let target = mhz.clamp(min, max);
        if target != mhz {
            debug!(
                "GPU: requested {mhz} MHz outside discovered range {min}..{max} MHz; \
                 writing the discovered bound {target} MHz"
            );
        }

        // Order matters: the software minimum first, then the user limit.
        self.card_write("gt_min_freq_mhz", min);
        self.card_write("gt_max_freq_mhz", target);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn card_root(tag: &str, files: &[(&str, &str)]) -> (SysfsRoot, PathBuf) {
        let dir = std::env::temp_dir().join(format!("ww_gpu_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let card = dir.join(DRM_BASE).join("card0");
        fs::create_dir_all(&card).unwrap();
        for (name, value) in files {
            fs::write(card.join(name), value).unwrap();
        }
        (SysfsRoot::new(dir), card)
    }

    #[test]
    fn card1_is_preferred_over_card0() {
        let dir = std::env::temp_dir().join(format!("ww_gpu_order_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for card in ["card0", "card1"] {
            fs::create_dir_all(dir.join(DRM_BASE).join(card)).unwrap();
            fs::write(
                dir.join(DRM_BASE).join(card).join("gt_max_freq_mhz"),
                "1000\n",
            )
            .unwrap();
        }
        let gpu = LinuxGpu::with_root(SysfsRoot::new(&dir)).unwrap();
        assert_eq!(gpu.card.as_ref().unwrap().file_name().unwrap(), "card1");
        assert!(gpu.is_supported());
    }

    #[test]
    fn no_card_is_unsupported_but_degrades_gracefully() {
        let dir = std::env::temp_dir().join(format!("ww_gpu_none_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(LinuxGpu::with_root(SysfsRoot::new(dir)).is_err());

        let gpu = LinuxGpu::with_card_path(PathBuf::from("/definitely/not/here"));
        assert_eq!(gpu.gpu_bounds().unwrap(), (300, 1100));
        assert_eq!(gpu.gpu_freq().unwrap(), 0);
        gpu.set_gpu_freq(900).unwrap(); // must not panic
    }

    #[test]
    fn bounds_fall_back_through_rpn_then_gt_min() {
        let (root, _card) = card_root(
            "fallback",
            &[("gt_max_freq_mhz", "900\n"), ("gt_min_freq_mhz", "250\n")],
        );
        let gpu = LinuxGpu::with_root(root).unwrap();
        assert_eq!(gpu.gpu_bounds().unwrap(), (250, 900));

        let (root, _card) = card_root("final", &[("gt_max_freq_mhz", "1000\n")]);
        let gpu = LinuxGpu::with_root(root).unwrap();
        assert_eq!(gpu.gpu_bounds().unwrap(), (300, 1000));
    }

    #[test]
    fn set_writes_min_then_max_with_clamping() {
        let (root, card) = card_root(
            "writes",
            &[
                ("gt_RPn_freq_mhz", "300\n"),
                ("gt_RP0_freq_mhz", "1100\n"),
                ("gt_min_freq_mhz", "300\n"),
                ("gt_max_freq_mhz", "1100\n"),
            ],
        );
        let gpu = LinuxGpu::with_root(root).unwrap();

        gpu.set_gpu_freq(900).unwrap();
        assert_eq!(
            fs::read_to_string(card.join("gt_min_freq_mhz")).unwrap(),
            "300"
        );
        assert_eq!(
            fs::read_to_string(card.join("gt_max_freq_mhz")).unwrap(),
            "900"
        );

        gpu.set_gpu_freq(99_999).unwrap();
        assert_eq!(
            fs::read_to_string(card.join("gt_min_freq_mhz")).unwrap(),
            "300"
        );
        assert_eq!(
            fs::read_to_string(card.join("gt_max_freq_mhz")).unwrap(),
            "1100"
        );

        gpu.set_gpu_freq(10).unwrap();
        assert_eq!(
            fs::read_to_string(card.join("gt_min_freq_mhz")).unwrap(),
            "300"
        );
        assert_eq!(
            fs::read_to_string(card.join("gt_max_freq_mhz")).unwrap(),
            "300"
        );
    }

    /// A degenerate (`min == max`) or incomplete range must never be written.
    #[test]
    fn degenerate_or_incomplete_range_is_never_written() {
        let (root, card) = card_root(
            "degenerate",
            &[
                ("gt_RPn_freq_mhz", "700\n"),
                ("gt_RP0_freq_mhz", "700\n"),
                ("gt_min_freq_mhz", "700\n"),
                ("gt_max_freq_mhz", "700\n"),
            ],
        );
        let gpu = LinuxGpu::with_root(root).unwrap();
        assert_eq!(gpu.discovered_gpu_bounds(), None);
        gpu.set_gpu_freq(900).unwrap();
        // Untouched: the file still holds the raw kernel content (with its newline).
        assert_eq!(
            fs::read_to_string(card.join("gt_max_freq_mhz")).unwrap(),
            "700\n"
        );
        assert_eq!(
            fs::read_to_string(card.join("gt_min_freq_mhz")).unwrap(),
            "700\n"
        );

        // Only a max exposed (no min): there is nothing to clamp against.
        let (root, card) = card_root("onlymax", &[("gt_max_freq_mhz", "1100\n")]);
        let gpu = LinuxGpu::with_root(root).unwrap();
        assert_eq!(gpu.discovered_gpu_bounds(), None);
        gpu.set_gpu_freq(900).unwrap();
        assert_eq!(
            fs::read_to_string(card.join("gt_max_freq_mhz")).unwrap(),
            "1100\n"
        );
    }

    /// Discovery for writes must ignore the nodes the controller itself writes
    /// (`gt_min_freq_mhz`/`gt_max_freq_mhz`): only the immutable `gt_RPn`/`gt_RP0`
    /// may define the range, otherwise it ratchets with every write.
    #[test]
    fn discovery_ignores_the_written_nodes() {
        // A card exposing only the writable pair: no immutable range -> never write.
        let (root, card) = card_root(
            "writtenonly",
            &[("gt_max_freq_mhz", "1100\n"), ("gt_min_freq_mhz", "300\n")],
        );
        let gpu = LinuxGpu::with_root(root).unwrap();
        assert_eq!(gpu.discovered_gpu_bounds(), None);
        assert_eq!(gpu.gpu_bounds().unwrap(), (300, 1100)); // display keeps Go fallbacks
        gpu.set_gpu_freq(900).unwrap();
        assert_eq!(
            fs::read_to_string(card.join("gt_max_freq_mhz")).unwrap(),
            "1100\n"
        );
        assert_eq!(
            fs::read_to_string(card.join("gt_min_freq_mhz")).unwrap(),
            "300\n"
        );
    }

    /// A card whose writable `gt_max_freq_mhz` was ratcheted *down* by an earlier write
    /// still discovers the true ceiling from `gt_RP0`, so the next write restores it
    /// instead of shrinking the range forever.
    #[test]
    fn ratcheted_written_node_does_not_shrink_the_range() {
        let (root, card) = card_root(
            "ratchet",
            &[
                ("gt_RPn_freq_mhz", "300\n"),
                ("gt_RP0_freq_mhz", "1100\n"),
                ("gt_min_freq_mhz", "300\n"),
                ("gt_max_freq_mhz", "500\n"), // left behind by an earlier write
            ],
        );
        let gpu = LinuxGpu::with_root(root).unwrap();
        assert_eq!(gpu.discovered_gpu_bounds(), Some((300, 1100)));
        gpu.set_gpu_freq(99_999).unwrap();
        assert_eq!(
            fs::read_to_string(card.join("gt_max_freq_mhz")).unwrap(),
            "1100"
        );
    }
}
