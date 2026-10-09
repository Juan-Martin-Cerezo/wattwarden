use crate::error::{Result, WattWardenError};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum PowerProfile {
    #[serde(rename = "Normal")]
    #[default]
    Normal,
    #[serde(rename = "Performance")]
    Performance,
    #[serde(rename = "Extreme")]
    Extreme,
    #[serde(rename = "Auto Extreme")]
    AutoExtreme,
}

impl std::fmt::Display for PowerProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "Normal"),
            Self::Performance => write!(f, "Performance"),
            Self::Extreme => write!(f, "Extreme"),
            Self::AutoExtreme => write!(f, "Auto Extreme"),
        }
    }
}

impl std::str::FromStr for PowerProfile {
    type Err = WattWardenError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().trim() {
            "normal" | "restore" | "default" | "restore/default" => Ok(Self::Normal),
            "performance" | "perf" => Ok(Self::Performance),
            "extreme" => Ok(Self::Extreme),
            "auto" | "autoextreme" | "auto-extreme" => Ok(Self::AutoExtreme),
            other => Err(WattWardenError::Config(format!(
                "Unknown profile '{}'. Valid options: normal, performance, extreme, auto",
                other
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AutoExtremeLevel {
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    #[default]
    High,
}

impl std::fmt::Display for AutoExtremeLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
        }
    }
}

impl std::str::FromStr for AutoExtremeLevel {
    type Err = WattWardenError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().trim() {
            "low" => Ok(Self::Low),
            "medium" | "med" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            other => Err(WattWardenError::Config(format!(
                "Unknown auto-extreme level '{}'. Valid options: low, medium, high",
                other
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LevelParams {
    pub idle_threshold: f64,
    pub idle_cores: usize,
    pub epp_idle: &'static str,
    pub epp_load: &'static str,
    pub turbo_from: f64,
    /// `false` = no escribir turbo/EPP en el lazo adaptativo (comportamiento historico = High).
    pub manage_power_hints: bool,
    /// Delta (en puntos de brillo) aplicado sobre `terminal_brightness`/`gui_brightness` del config.
    pub brightness_delta: i16,
}

impl AutoExtremeLevel {
    pub fn params(&self) -> LevelParams {
        let ncpu = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(1);
        match self {
            Self::High => LevelParams {
                idle_threshold: 0.3,
                idle_cores: 2,
                epp_idle: "power",
                epp_load: "power",
                turbo_from: 0.8,
                manage_power_hints: false,
                brightness_delta: 0,
            },
            Self::Medium => LevelParams {
                idle_threshold: 0.5,
                idle_cores: (ncpu / 2).max(2),
                epp_idle: "power",
                epp_load: "power",
                turbo_from: 0.67,
                manage_power_hints: true,
                brightness_delta: 10,
            },
            Self::Low => LevelParams {
                idle_threshold: 0.7,
                idle_cores: 0,
                epp_idle: "balance_power",
                epp_load: "balance_performance",
                turbo_from: 0.33,
                manage_power_hints: true,
                brightness_delta: 20,
            },
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::Low,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Opt-in puro (Juan): sin ajuste del usuario el daemon no escribe nada.
    /// Todos los flags empiezan apagados y `profile`/`battery_charge_limit`
    /// en `None` ("el usuario no eligió nada" = no aplicar nada).
    #[serde(default)]
    pub auto_extreme_enabled: bool,
    #[serde(default)]
    pub auto_extreme_level: AutoExtremeLevel,
    #[serde(default)]
    pub auto_brightness: bool,
    #[serde(default)]
    pub profile: Option<PowerProfile>,
    #[serde(default)]
    pub battery_charge_limit: Option<u8>,
    #[serde(default = "default_terminal_brightness")]
    pub terminal_brightness: u8,
    #[serde(default = "default_gui_brightness")]
    pub gui_brightness: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auto_extreme_enabled: false,
            auto_extreme_level: AutoExtremeLevel::High,
            auto_brightness: false,
            profile: None,
            battery_charge_limit: None,
            terminal_brightness: 25,
            gui_brightness: 65,
        }
    }
}

fn default_terminal_brightness() -> u8 {
    25
}

fn default_gui_brightness() -> u8 {
    65
}

impl Config {
    pub fn default_path() -> PathBuf {
        if cfg!(windows) {
            let program_data =
                std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into());
            PathBuf::from(program_data)
                .join("wattwarden")
                .join("config.json")
        } else {
            PathBuf::from("/etc/wattwarden/config.json")
        }
    }

    pub fn load_or_default(path: Option<&Path>) -> Self {
        let p = path.map(PathBuf::from).unwrap_or_else(Self::default_path);
        match fs::read_to_string(&p) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: Option<&Path>) -> Result<()> {
        let p = path.map(PathBuf::from).unwrap_or_else(Self::default_path);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).map_err(|e| WattWardenError::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }

        let json = serde_json::to_string_pretty(self)?;
        fs::write(&p, json).map_err(|e| WattWardenError::Io { path: p, source: e })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_extreme_level_default_is_high() {
        assert_eq!(AutoExtremeLevel::default(), AutoExtremeLevel::High);
        assert_eq!(Config::default().auto_extreme_level, AutoExtremeLevel::High);
    }

    #[test]
    fn test_auto_extreme_level_deserialize_lowercase() {
        let low: AutoExtremeLevel = serde_json::from_str("\"low\"").unwrap();
        let medium: AutoExtremeLevel = serde_json::from_str("\"medium\"").unwrap();
        let high: AutoExtremeLevel = serde_json::from_str("\"high\"").unwrap();
        assert_eq!(low, AutoExtremeLevel::Low);
        assert_eq!(medium, AutoExtremeLevel::Medium);
        assert_eq!(high, AutoExtremeLevel::High);
    }

    #[test]
    fn test_auto_extreme_level_from_str() {
        assert_eq!(
            "low".parse::<AutoExtremeLevel>().unwrap(),
            AutoExtremeLevel::Low
        );
        assert_eq!(
            "MEDIUM".parse::<AutoExtremeLevel>().unwrap(),
            AutoExtremeLevel::Medium
        );
        assert_eq!(
            "high".parse::<AutoExtremeLevel>().unwrap(),
            AutoExtremeLevel::High
        );
        assert!("turbo".parse::<AutoExtremeLevel>().is_err());
    }

    #[test]
    fn test_auto_extreme_level_cycle() {
        assert_eq!(AutoExtremeLevel::Low.next(), AutoExtremeLevel::Medium);
        assert_eq!(AutoExtremeLevel::Medium.next(), AutoExtremeLevel::High);
        assert_eq!(AutoExtremeLevel::High.next(), AutoExtremeLevel::Low);
    }

    #[test]
    fn test_level_params_ordering() {
        let ncpu = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(1)
            .max(1);
        let effective_cores = |p: LevelParams| {
            if p.idle_cores == 0 {
                ncpu
            } else {
                p.idle_cores.min(ncpu)
            }
        };

        let high = AutoExtremeLevel::High.params();
        let medium = AutoExtremeLevel::Medium.params();
        let low = AutoExtremeLevel::Low.params();

        assert!(high.idle_threshold < medium.idle_threshold);
        assert!(medium.idle_threshold < low.idle_threshold);
        assert!(effective_cores(high) <= effective_cores(medium));
        assert!(effective_cores(medium) <= effective_cores(low));
        assert_eq!(effective_cores(low), ncpu);

        assert_eq!(high.turbo_from, 0.8);
        assert_eq!(medium.turbo_from, 0.67);
        assert_eq!(low.turbo_from, 0.33);

        assert_eq!(high.epp_idle, "power");
        assert_eq!(low.epp_idle, "balance_power");
        assert_eq!(low.epp_load, "balance_performance");
    }

    /// High tiene que ser el comportamiento historico: el lazo NO toca turbo/EPP
    /// y el brillo sale del config del usuario sin correccion.
    #[test]
    fn test_high_level_is_behavior_preserving() {
        let high = AutoExtremeLevel::High.params();
        assert!(!high.manage_power_hints);
        assert_eq!(high.brightness_delta, 0);
        assert_eq!(high.idle_threshold, 0.3);
        assert_eq!(high.idle_cores, 2);

        assert!(AutoExtremeLevel::Medium.params().manage_power_hints);
        assert!(AutoExtremeLevel::Low.params().manage_power_hints);
        assert!(
            AutoExtremeLevel::Medium.params().brightness_delta
                > AutoExtremeLevel::High.params().brightness_delta
        );
    }

    #[test]
    fn test_config_without_level_field_defaults_high() {
        let cfg: Config = serde_json::from_str(r#"{"profile":"Normal"}"#).unwrap();
        assert_eq!(cfg.auto_extreme_level, AutoExtremeLevel::High);
    }

    /// Opt-in puro (Juan): una config sin campos no habilita nada. Una config
    /// vieja que diga `"profile": "Normal"` explícito sigue aplicándolo
    /// (compatibilidad: el valor escrito gana sobre el default `None`).
    #[test]
    fn test_go_config_json_round_trip() {
        let go_json = r#"{"auto_extreme_enabled":true,"auto_brightness":false}"#;
        let cfg: Config = serde_json::from_str(go_json).unwrap();
        assert!(cfg.auto_extreme_enabled);
        assert!(!cfg.auto_brightness);

        let missing_keys: Config = serde_json::from_str(r#"{"profile":"Normal"}"#).unwrap();
        assert!(!missing_keys.auto_extreme_enabled);
        assert!(!missing_keys.auto_brightness);
        assert_eq!(missing_keys.profile, Some(PowerProfile::Normal));

        let empty: Config = serde_json::from_str(r#"{}"#).unwrap();
        assert!(!empty.auto_extreme_enabled);
        assert!(!empty.auto_brightness);
        assert_eq!(empty.profile, None);
        assert_eq!(empty.battery_charge_limit, None);

        let written = serde_json::to_string(&Config::default()).unwrap();
        let back: serde_json::Value = serde_json::from_str(&written).unwrap();
        assert_eq!(back["auto_extreme_enabled"], serde_json::Value::Bool(false));
        assert_eq!(back["auto_brightness"], serde_json::Value::Bool(false));
    }

    #[test]
    fn test_default_config_enables_nothing() {
        let cfg = Config::default();
        assert!(!cfg.auto_extreme_enabled);
        assert!(!cfg.auto_brightness);
        assert_eq!(cfg.profile, None);
        assert_eq!(cfg.battery_charge_limit, None);
    }

    #[test]
    fn test_explicit_normal_profile_round_trip() {
        let cfg: Config = serde_json::from_str(r#"{"profile":"Normal"}"#).unwrap();
        assert_eq!(cfg.profile, Some(PowerProfile::Normal));
        let written = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&written).unwrap();
        assert_eq!(back.profile, Some(PowerProfile::Normal));
    }

    #[test]
    fn test_config_level_round_trip() {
        let tmp_dir = std::env::temp_dir().join(format!("ww_test_level_{}", std::process::id()));
        let cfg_path = tmp_dir.join("level_config.json");

        let cfg = Config {
            auto_extreme_level: AutoExtremeLevel::Low,
            ..Default::default()
        };
        cfg.save(Some(&cfg_path)).expect("save should succeed");

        let loaded = Config::load_or_default(Some(&cfg_path));
        assert_eq!(loaded.auto_extreme_level, AutoExtremeLevel::Low);

        let _ = std::fs::remove_dir_all(tmp_dir);
    }
}
