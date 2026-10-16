//! Guided battery gauge recalibration (opt-in, CLI-driven, multi-phase).
//!
//! The fuel gauge behind `charge_full` learns by watching full charges. With a
//! charge ceiling (e.g. 50–80%) it never sees one, so its capacity estimate can
//! drift downwards and reported "battery health" degrades faster than the
//! chemistry does. A calibration cycle lifts the ceiling to 100%, walks the user
//! through one full discharge/recharge so the gauge relearns, then restores the
//! previous threshold. Nothing here runs automatically: every phase is a user
//! command (WattWarden obeys the user, never the cable).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use wattwarden_core::{ChargeThreshold, PowerSource};

pub const STATE_PATH: &str = "/var/lib/wattwarden/calibration.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// Charge to 100% with the ceiling lifted.
    Charge,
    /// Discharge to 10% or below on battery.
    Discharge,
    /// Recharge to 100%; the gauge relearns full capacity here.
    Recharge,
}

impl Phase {
    pub fn name(&self) -> &'static str {
        match self {
            Phase::Charge => "charge",
            Phase::Discharge => "discharge",
            Phase::Recharge => "recharge",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationState {
    /// Threshold that was active before `start`; restored by `finish`.
    pub saved_threshold: Option<u8>,
    pub phase: Phase,
}

impl CalibrationState {
    pub fn load(path: &Path) -> Option<Self> {
        let content = fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(path, json).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn clear(path: &Path) {
        let _ = fs::remove_file(path);
    }

    /// Advances the phase machine from the observed gauge state.
    pub fn advance(&mut self, capacity: u8, charging: bool) {
        match self.phase {
            Phase::Charge if capacity >= 100 => self.phase = Phase::Discharge,
            Phase::Discharge if !charging && capacity <= 10 => self.phase = Phase::Recharge,
            _ => {}
        }
    }

    pub fn guidance(&self) -> &'static str {
        match self.phase {
            Phase::Charge => {
                "Next: plug in and charge to 100%, then run `sudo wattwarden calibrate status`."
            }
            Phase::Discharge => {
                "Next: unplug and use the machine on battery down to 10% or below, then re-check."
            }
            Phase::Recharge => {
                "Next: recharge to 100% now, then run `sudo wattwarden calibrate done`."
            }
        }
    }
}

/// Phase 1: lift the ceiling to 100% and remember what it was.
pub fn start(threshold: &dyn ChargeThreshold, state_path: &Path) -> Result<String, String> {
    if CalibrationState::load(state_path).is_some() {
        return Err(
            "calibration already in progress (see `sudo wattwarden calibrate status`, or `abort`)"
                .to_string(),
        );
    }
    if !threshold.supports_threshold() {
        return Err(
            "hardware does not expose a charge threshold; calibration requires one".to_string(),
        );
    }
    let saved_threshold = threshold.charge_threshold().ok();
    threshold
        .set_charge_threshold(100)
        .map_err(|e| format!("failed to lift the charge ceiling: {e}"))?;

    let state = CalibrationState {
        saved_threshold,
        phase: Phase::Charge,
    };
    state.save(state_path)?;
    Ok(format!(
        "Calibration started: charge ceiling lifted to 100% (was: {}).\n{}",
        saved_threshold
            .map(|t| format!("{t}%"))
            .unwrap_or_else(|| "unknown".to_string()),
        state.guidance()
    ))
}

/// Phase walking: observe the gauge and tell the user what comes next.
pub fn status(battery: &dyn PowerSource, state_path: &Path) -> Result<String, String> {
    let mut state = CalibrationState::load(state_path).ok_or_else(|| {
        "no calibration in progress (start one with `sudo wattwarden calibrate start`)".to_string()
    })?;
    let capacity = battery.battery_percentage().map_err(|e| e.to_string())?;
    let charging = battery.is_charging().unwrap_or(false);
    state.advance(capacity, charging);
    state.save(state_path)?;
    Ok(format!(
        "Calibration in progress: phase {} at {capacity}% ({}).\n{}",
        state.phase.name(),
        if charging { "charging" } else { "on battery" },
        state.guidance()
    ))
}

/// Final phase: restore the previous threshold and forget the state.
pub fn finish(threshold: &dyn ChargeThreshold, state_path: &Path) -> Result<String, String> {
    let state = CalibrationState::load(state_path).ok_or_else(|| {
        "no calibration in progress (start one with `sudo wattwarden calibrate start`)".to_string()
    })?;
    match state.saved_threshold {
        Some(t) => {
            threshold
                .set_charge_threshold(t)
                .map_err(|e| format!("failed to restore the {t}% threshold: {e}"))?;
            CalibrationState::clear(state_path);
            Ok(format!(
                "Calibration finished: previous charge threshold restored at {t}%."
            ))
        }
        None => {
            CalibrationState::clear(state_path);
            Ok("Calibration finished. The previous threshold was unknown, so the ceiling stays at 100% — set one with `sudo wattwarden charge-limit <55-100>` if desired.".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
    use std::sync::Mutex;

    struct FakeThreshold {
        supported: bool,
        current: Mutex<Option<u8>>,
        writes: Mutex<Vec<u8>>,
    }

    impl FakeThreshold {
        fn new(supported: bool, current: Option<u8>) -> Self {
            Self {
                supported,
                current: Mutex::new(current),
                writes: Mutex::new(Vec::new()),
            }
        }
    }

    impl ChargeThreshold for FakeThreshold {
        fn supports_threshold(&self) -> bool {
            self.supported
        }
        fn charge_threshold(&self) -> wattwarden_core::Result<u8> {
            self.current
                .lock()
                .unwrap()
                .ok_or_else(|| wattwarden_core::WattWardenError::Unsupported("no threshold".into()))
        }
        fn set_charge_threshold(&self, threshold: u8) -> wattwarden_core::Result<()> {
            if !self.supported {
                return Err(wattwarden_core::WattWardenError::Unsupported(
                    "no threshold".into(),
                ));
            }
            *self.current.lock().unwrap() = Some(threshold);
            self.writes.lock().unwrap().push(threshold);
            Ok(())
        }
    }

    struct FakeBattery {
        capacity: AtomicU8,
        charging: AtomicBool,
    }

    impl FakeBattery {
        fn new(capacity: u8, charging: bool) -> Self {
            Self {
                capacity: AtomicU8::new(capacity),
                charging: AtomicBool::new(charging),
            }
        }
    }

    impl PowerSource for FakeBattery {
        fn battery_percentage(&self) -> wattwarden_core::Result<u8> {
            Ok(self.capacity.load(Ordering::Relaxed))
        }
        fn is_charging(&self) -> wattwarden_core::Result<bool> {
            Ok(self.charging.load(Ordering::Relaxed))
        }
        fn consumption_watts(&self) -> wattwarden_core::Result<f64> {
            Ok(0.0)
        }
        fn time_remaining(&self) -> wattwarden_core::Result<String> {
            Ok("Calculating...".into())
        }
    }

    fn state_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ww_cal_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("calibration.json")
    }

    #[test]
    fn full_cycle_lifts_walks_and_restores() {
        let path = state_path("cycle");
        let threshold = FakeThreshold::new(true, Some(80));
        let battery = FakeBattery::new(55, true);

        let msg = start(&threshold, &path).unwrap();
        assert!(msg.contains("lifted to 100% (was: 80%)"));
        assert_eq!(threshold.writes.lock().unwrap().as_slice(), &[100]);

        // Still charging under 100: phase stays `charge`.
        let msg = status(&battery, &path).unwrap();
        assert!(msg.contains("phase charge at 55%"));

        // Full while charging: advance to discharge.
        battery.capacity.store(100, Ordering::Relaxed);
        let msg = status(&battery, &path).unwrap();
        assert!(msg.contains("phase discharge at 100%"));

        // 5% on battery: advance to recharge.
        battery.capacity.store(5, Ordering::Relaxed);
        battery.charging.store(false, Ordering::Relaxed);
        let msg = status(&battery, &path).unwrap();
        assert!(msg.contains("phase recharge at 5%"));

        let msg = finish(&threshold, &path).unwrap();
        assert!(msg.contains("restored at 80%"));
        assert_eq!(threshold.writes.lock().unwrap().as_slice(), &[100, 80]);
        assert!(CalibrationState::load(&path).is_none());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn start_twice_is_rejected() {
        let path = state_path("twice");
        let threshold = FakeThreshold::new(true, Some(80));
        start(&threshold, &path).unwrap();
        let err = start(&threshold, &path).unwrap_err();
        assert!(err.contains("already in progress"));
        CalibrationState::clear(&path);
    }

    #[test]
    fn unsupported_hardware_is_rejected() {
        let path = state_path("unsupported");
        let threshold = FakeThreshold::new(false, None);
        let err = start(&threshold, &path).unwrap_err();
        assert!(err.contains("does not expose a charge threshold"));
        assert!(threshold.writes.lock().unwrap().is_empty());
        assert!(CalibrationState::load(&path).is_none());
    }

    #[test]
    fn status_and_finish_without_state_error() {
        let path = state_path("nostate");
        let battery = FakeBattery::new(50, false);
        let threshold = FakeThreshold::new(true, Some(80));
        assert!(status(&battery, &path)
            .unwrap_err()
            .contains("no calibration"));
        assert!(finish(&threshold, &path)
            .unwrap_err()
            .contains("no calibration"));
        assert!(threshold.writes.lock().unwrap().is_empty());
    }

    /// Without a readable previous threshold, `finish` must NOT invent one:
    /// it leaves the ceiling at 100% and says so.
    #[test]
    fn finish_without_saved_threshold_writes_nothing() {
        let path = state_path("nosaved");
        let threshold = FakeThreshold::new(true, None);
        start(&threshold, &path).unwrap();
        let msg = finish(&threshold, &path).unwrap();
        assert!(msg.contains("ceiling stays at 100%"));
        assert_eq!(threshold.writes.lock().unwrap().as_slice(), &[100]);
    }
}
