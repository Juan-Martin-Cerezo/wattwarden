pub mod config;
pub mod error;
pub mod traits;
pub mod typestate;

pub use config::{AutoExtremeLevel, Config, LevelParams, PowerProfile};
pub use error::{Result, WattWardenError};
pub use traits::*;
pub use typestate::{DeviceGovernor, Extreme, Normal, Performance};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_from_str() {
        assert_eq!(
            "performance".parse::<PowerProfile>().unwrap(),
            PowerProfile::Performance
        );
        assert_eq!(
            "perf".parse::<PowerProfile>().unwrap(),
            PowerProfile::Performance
        );
        assert_eq!(
            "extreme".parse::<PowerProfile>().unwrap(),
            PowerProfile::Extreme
        );
        assert_eq!(
            "normal".parse::<PowerProfile>().unwrap(),
            PowerProfile::Normal
        );
        assert_eq!(
            "restore".parse::<PowerProfile>().unwrap(),
            PowerProfile::Normal
        );
        assert_eq!(
            "default".parse::<PowerProfile>().unwrap(),
            PowerProfile::Normal
        );
        assert_eq!(
            "auto".parse::<PowerProfile>().unwrap(),
            PowerProfile::AutoExtreme
        );
        assert_eq!(
            "auto-extreme".parse::<PowerProfile>().unwrap(),
            PowerProfile::AutoExtreme
        );
        assert_eq!(
            "autoextreme".parse::<PowerProfile>().unwrap(),
            PowerProfile::AutoExtreme
        );
        assert!("invalid".parse::<PowerProfile>().is_err());
    }

    #[test]
    fn test_profile_display_and_serde() {
        let profiles = [
            PowerProfile::Normal,
            PowerProfile::Performance,
            PowerProfile::Extreme,
            PowerProfile::AutoExtreme,
        ];
        for p in &profiles {
            let json = serde_json::to_string(p).expect("serialization should succeed");
            let deserialized: PowerProfile =
                serde_json::from_str(&json).expect("deserialization should succeed");
            assert_eq!(*p, deserialized);
            assert_eq!(
                format!("{}", p),
                format!("{:?}", p).replace("AutoExtreme", "Auto Extreme")
            );
        }
    }

    #[test]
    fn test_config_save_load() {
        let tmp_dir = std::env::temp_dir().join(format!("ww_test_cfg_{}", std::process::id()));
        let cfg_path = tmp_dir.join("test_config.json");

        let mut cfg = Config::default();
        assert_eq!(cfg.profile, None);
        assert!(!cfg.auto_brightness);
        assert!(!cfg.auto_extreme_enabled);
        assert_eq!(cfg.battery_charge_limit, None);

        cfg.profile = Some(PowerProfile::Extreme);
        cfg.auto_brightness = false;
        cfg.terminal_brightness = 30;
        cfg.gui_brightness = 70;
        cfg.battery_charge_limit = Some(85);

        cfg.save(Some(&cfg_path)).expect("save should succeed");

        let loaded = Config::load_or_default(Some(&cfg_path));
        assert_eq!(loaded.profile, Some(PowerProfile::Extreme));
        assert!(!loaded.auto_brightness);
        assert_eq!(loaded.terminal_brightness, 30);
        assert_eq!(loaded.gui_brightness, 70);
        assert_eq!(loaded.battery_charge_limit, Some(85));

        let _ = std::fs::remove_dir_all(tmp_dir);
    }

    #[test]
    fn test_typestate_transitions() {
        let governor = DeviceGovernor::new();
        let perf = governor.to_performance();
        let extreme = perf.to_extreme();
        let normal = extreme.to_normal();
        let perf2 = normal.to_performance();
        let normal2 = perf2.to_normal();
        let extreme2 = normal2.to_extreme();
        let perf3 = extreme2.to_performance();
        let _ = perf3.to_normal();
    }

    #[test]
    fn test_error_display() {
        let err = WattWardenError::InterfaceNotFound("test_interface".into());
        assert!(format!("{}", err).contains("test_interface"));

        let err_perm = WattWardenError::PermissionDenied("test_action".into());
        assert!(format!("{}", err_perm).contains("root/administrator"));

        let err_bounds = WattWardenError::OutOfBounds {
            value: 120,
            min: 50,
            max: 100,
        };
        assert!(format!("{}", err_bounds).contains("120"));
    }
}
