//! Battery ageing telemetry: full vs design capacity, cycle count and temperature.
//!
//! Every field degrades gracefully: hardware that does not expose a metric reports
//! `None`, never a guess. The percentage is the *reported* health of the pack — on
//! packs that never see a full charge (a 50–80% charge ceiling) the fuel gauge can
//! drift downwards, so a calibration cycle is required before trusting the number.

use serde::{Deserialize, Serialize};

/// Unit backing the full/design capacity pair reported by the hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthUnit {
    /// Microamp-hours (`charge_full` / `charge_full_design`).
    ChargeUah,
    /// Micro-watt-hours (`energy_full` / `energy_full_design`).
    EnergyUwh,
}

impl HealthUnit {
    /// Label used when the raw micro values are scaled for display (mAh / mWh).
    pub fn display_label(&self) -> &'static str {
        match self {
            HealthUnit::ChargeUah => "mAh",
            HealthUnit::EnergyUwh => "mWh",
        }
    }

    /// Micro units -> display units (÷ 1000).
    pub fn scale_to_display(&self, micro: u64) -> u64 {
        micro / 1000
    }
}

/// Snapshot of the battery's ageing state.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BatteryHealth {
    /// False on desktops, servers, containers and machines without a battery node.
    pub present: bool,
    /// Full charge capacity the gauge currently reports (micro units).
    pub full: Option<u64>,
    /// Design capacity of the pack (micro units).
    pub design: Option<u64>,
    /// Unit of `full`/`design`; `None` when neither pair is exposed.
    pub unit: Option<HealthUnit>,
    /// Charge/discharge cycles reported by the firmware (often absent).
    pub cycle_count: Option<u32>,
    /// Pack temperature in °C, when the driver exposes one.
    pub temp_c: Option<f64>,
    pub model: Option<String>,
    pub manufacturer: Option<String>,
    pub serial: Option<String>,
}

impl BatteryHealth {
    /// `full / design * 100`, one decimal. `None` when either side is missing
    /// or the design value is nonsense.
    pub fn health_pct(&self) -> Option<f64> {
        match (self.full, self.design) {
            (Some(f), Some(d)) if d > 0 => Some(((f as f64) * 1000.0 / d as f64).round() / 10.0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Vostro 5471 pack: 2068000 / 3684000 µAh.
    #[test]
    fn test_health_pct_vostro_rounds_to_one_decimal() {
        let h = BatteryHealth {
            present: true,
            full: Some(2_068_000),
            design: Some(3_684_000),
            unit: Some(HealthUnit::ChargeUah),
            ..Default::default()
        };
        assert_eq!(h.health_pct(), Some(56.1));
    }

    #[test]
    fn test_health_pct_needs_both_sides() {
        let only_full = BatteryHealth {
            full: Some(2_068_000),
            ..Default::default()
        };
        assert_eq!(only_full.health_pct(), None);

        let only_design = BatteryHealth {
            design: Some(3_684_000),
            ..Default::default()
        };
        assert_eq!(only_design.health_pct(), None);

        let zero_design = BatteryHealth {
            full: Some(100),
            design: Some(0),
            ..Default::default()
        };
        assert_eq!(zero_design.health_pct(), None);

        assert_eq!(BatteryHealth::default().health_pct(), None);
    }

    #[test]
    fn test_unit_display_scaling() {
        assert_eq!(HealthUnit::ChargeUah.display_label(), "mAh");
        assert_eq!(HealthUnit::EnergyUwh.display_label(), "mWh");
        assert_eq!(HealthUnit::ChargeUah.scale_to_display(2_068_000), 2068);
    }
}
