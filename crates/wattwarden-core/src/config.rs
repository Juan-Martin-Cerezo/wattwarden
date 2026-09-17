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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub auto_extreme_enabled: bool,
    #[serde(default = "default_auto_brightness")]
    pub auto_brightness: bool,
    #[serde(default)]
    pub profile: PowerProfile,
    #[serde(default = "default_battery_limit")]
    pub battery_charge_limit: Option<u8>,
    #[serde(default = "default_terminal_brightness")]
    pub terminal_brightness: u8,
    #[serde(default = "default_gui_brightness")]
    pub gui_brightness: u8,
}

fn default_auto_brightness() -> bool {
    true
}

fn default_battery_limit() -> Option<u8> {
    Some(80)
}

fn default_terminal_brightness() -> u8 {
    25
}

fn default_gui_brightness() -> u8 {
    65
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auto_extreme_enabled: false,
            auto_brightness: true,
            profile: PowerProfile::Normal,
            battery_charge_limit: Some(80),
            terminal_brightness: 25,
            gui_brightness: 65,
        }
    }
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
