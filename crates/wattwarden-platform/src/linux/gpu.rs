//! Integrated GPU frequency control via the DRM `gt_*` sysfs knobs.
//!
//! Hardware-agnostic rewrite of the Go transcription:
//!
//! * `getGPUPath()` -> the first DRM card exposing a writable frequency node, in Go's
//!   preference order (`card1`, then `card0`, then every other `cardN`), probed at the
//!   card root (`cardN/gt_max_freq_mhz`, the Intel i915/xe layout) and under its
//!   `device/` link. No frequency node on any card (ARM/v3d, AMD DPM-only, a server
//!   with no GPU) means "no support": [`LinuxGpu::with_root`] returns
//!   `InterfaceNotFound` and the backend runs without GPU control. Nothing is ever
//!   written to a node that does not exist.
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

/// Writable GPU frequency node probed on every DRM card (Intel i915/xe).
const GT_MAX_FREQ: &str = "gt_max_freq_mhz";

/// Go fallbacks for `GetGPUBounds`.
pub const FALLBACK_MIN_MHZ: u32 = 300;
pub const FALLBACK_MAX_MHZ: u32 = 1100;

/// True for a DRM card directory (`card0`, `card1`, …) and false for its connector
/// entries (`card1-HDMI-A-1`, `card1-Writeback-2`, `renderD128`, `version`).
fn is_card_dir(name: &str) -> bool {
    name.strip_prefix("card")
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

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
        let Some(card) = card else {
            debug!(
                "GPU: no DRM card exposes a {GT_MAX_FREQ} node under /{DRM_BASE}; \
                 GPU controls disabled"
            );
            return Err(WattWardenError::InterfaceNotFound(
                "No controllable GPU frequency interface discovered in /sys/class/drm".into(),
            ));
        };
        Ok(Self {
            root,
            card: Some(card),
        })
    }

    /// Compatibility constructor: point the controller at an explicit absolute card
    /// path (used by tests and by callers that already resolved the node).
    pub fn with_card_path(card_path: PathBuf) -> Self {
        Self {
            root: SysfsRoot::new("/"),
            card: Some(card_path),
        }
    }

    /// Discovers the directory of the first DRM card exposing a writable frequency
    /// node, in Go's preference order (`card1`, then `card0`, then the remaining
    /// `cardN` sorted).
    ///
    /// The node is probed at the card root (`cardN/gt_max_freq_mhz`, the Intel layout)
    /// and under its `device/` link, so both kernel layouts are found. Cards carrying
    /// no frequency node are skipped, and no card at all yields `None`.
    fn discover(root: &SysfsRoot) -> Option<PathBuf> {
        Self::ordered_cards(root).into_iter().find_map(|card| {
            let rel = format!("{DRM_BASE}/{card}");
            [rel.clone(), format!("{rel}/device")]
                .into_iter()
                .find(|base| root.exists(&format!("{base}/{GT_MAX_FREQ}")))
                .map(|base| root.path(&base))
        })
    }

    /// DRM card directory names: Go's `card1`/`card0` first, then every other
    /// `cardN` in sorted order. Connector entries are excluded.
    fn ordered_cards(root: &SysfsRoot) -> Vec<String> {
        let cards: Vec<String> = root
            .names(DRM_BASE)
            .into_iter()
            .filter(|name| is_card_dir(name))
            .collect();
        let mut ordered: Vec<String> = Vec::new();
        for preferred in ["card1", "card0"] {
            if cards.iter().any(|c| c == preferred) {
                ordered.push(preferred.to_string());
            }
        }
        for card in cards {
            if !ordered.contains(&card) {
                ordered.push(card);
            }
        }
        ordered
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

    /// Best-effort write that only touches a node the kernel actually exposes. The
    /// write set is `gt_min_freq_mhz` + `gt_max_freq_mhz`; a kernel exposing neither
    /// (the discovery found the node elsewhere) must not have them created by a plain
    /// `fs::write`.
    fn card_write(&self, leaf: &str, value: u32) {
        let Some(card) = self.card.as_ref() else {
            return;
        };
        let path = card.join(leaf);
        if path.exists() {
            self.root.write_best_effort_path(&path, &value.to_string());
        } else {
            debug!(
                "GPU: {} is not exposed by the kernel; not writing it",
                path.display()
            );
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
        Ok(self.card_read(GT_MAX_FREQ).unwrap_or(0))
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

    /// The probe is not limited to `card0`/`card1`: any `cardN` is considered, so a
    /// machine whose only GPU node lives on `card2` is still discovered.
    #[test]
    fn a_card_other_than_card0_or_card1_is_discovered() {
        let dir = std::env::temp_dir().join(format!("ww_gpu_card2_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let card = dir.join(DRM_BASE).join("card2");
        fs::create_dir_all(&card).unwrap();
        fs::write(card.join("gt_RPn_freq_mhz"), "300\n").unwrap();
        fs::write(card.join("gt_RP0_freq_mhz"), "1300\n").unwrap();
        fs::write(card.join("gt_min_freq_mhz"), "300\n").unwrap();
        fs::write(card.join("gt_max_freq_mhz"), "1300\n").unwrap();

        let gpu = LinuxGpu::with_root(SysfsRoot::new(&dir)).unwrap();
        assert_eq!(gpu.card.as_ref().unwrap().file_name().unwrap(), "card2");
        assert_eq!(gpu.discovered_gpu_bounds(), Some((300, 1300)));
        gpu.set_gpu_freq(99_999).unwrap();
        assert_eq!(
            fs::read_to_string(card.join("gt_max_freq_mhz")).unwrap(),
            "1300"
        );
    }

    /// The frequency node is also probed under the card's `device/` link, which is
    /// where some kernels publish it instead of at the card root.
    #[test]
    fn the_frequency_node_is_found_under_the_device_link() {
        let dir = std::env::temp_dir().join(format!("ww_gpu_devlink_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let device = dir.join(DRM_BASE).join("card0").join("device");
        fs::create_dir_all(&device).unwrap();
        fs::write(device.join("gt_RPn_freq_mhz"), "200\n").unwrap();
        fs::write(device.join("gt_RP0_freq_mhz"), "900\n").unwrap();
        fs::write(device.join("gt_min_freq_mhz"), "200\n").unwrap();
        fs::write(device.join("gt_max_freq_mhz"), "900\n").unwrap();

        let gpu = LinuxGpu::with_root(SysfsRoot::new(&dir)).unwrap();
        assert_eq!(gpu.discovered_gpu_bounds(), Some((200, 900)));
        gpu.set_gpu_freq(500).unwrap();
        assert_eq!(
            fs::read_to_string(device.join("gt_max_freq_mhz")).unwrap(),
            "500"
        );
    }

    /// Connector entries (`card0-HDMI-A-1`, `renderD128`, `version`, …) are not GPU
    /// cards: a DRM class holding only those must report "unsupported" instead of
    /// pointing at a connector directory.
    #[test]
    fn connector_entries_are_not_mistaken_for_cards() {
        let dir = std::env::temp_dir().join(format!("ww_gpu_connectors_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for entry in ["card0-HDMI-A-1", "card1-Writeback-1", "renderD128"] {
            fs::create_dir_all(dir.join(DRM_BASE).join(entry)).unwrap();
            fs::write(
                dir.join(DRM_BASE).join(entry).join("gt_max_freq_mhz"),
                "900\n",
            )
            .unwrap();
        }
        fs::create_dir_all(dir.join(DRM_BASE)).unwrap();
        fs::write(dir.join(DRM_BASE).join("version"), "drm 1.1.0\n").unwrap();

        assert!(LinuxGpu::with_root(SysfsRoot::new(&dir)).is_err());
    }

    /// A write never creates a node the kernel does not expose: when the discovery
    /// found `gt_max_freq_mhz` on a card whose `gt_min_freq_mhz` is absent, only the
    /// existing node is touched.
    #[test]
    fn a_write_never_creates_a_missing_writable_node() {
        let (root, card) = card_root(
            "nowritecreate",
            &[
                ("gt_RPn_freq_mhz", "300\n"),
                ("gt_RP0_freq_mhz", "1100\n"),
                ("gt_max_freq_mhz", "1100\n"),
            ],
        );
        let gpu = LinuxGpu::with_root(root).unwrap();
        assert_eq!(gpu.discovered_gpu_bounds(), Some((300, 1100)));
        gpu.set_gpu_freq(700).unwrap();

        assert_eq!(
            fs::read_to_string(card.join("gt_max_freq_mhz")).unwrap(),
            "700"
        );
        assert!(
            !card.join("gt_min_freq_mhz").exists(),
            "the missing node must not be created"
        );
    }
}
