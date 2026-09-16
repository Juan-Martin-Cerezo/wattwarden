use std::fs;
use std::path::Path;
use std::process::Command;
use wattwarden_core::{Result, WattWardenError};

const SYSTEMD_UNIT_PATH: &str = "/etc/systemd/system/wattwarden.service";

pub fn install_systemd_service(binary_path: &Path) -> Result<()> {
    let unit_content = format!(
        r#"[Unit]
Description=WattWarden - Autonomous Hardware Power & Auto-Brightness Daemon
After=multi-user.target

[Service]
Type=simple
ExecStart={} daemon
Restart=always
RestartSec=5s
Environment=WATTWARDEN_DAEMON=1

[Install]
WantedBy=multi-user.target
"#,
        binary_path.display()
    );

    fs::write(SYSTEMD_UNIT_PATH, unit_content).map_err(|e| WattWardenError::Io {
        path: SYSTEMD_UNIT_PATH.into(),
        source: e,
    })?;

    let _ = Command::new("systemctl").arg("daemon-reload").status();
    let status = Command::new("systemctl")
        .args(["enable", "--now", "wattwarden"])
        .status()
        .map_err(|e| WattWardenError::Io {
            path: SYSTEMD_UNIT_PATH.into(),
            source: e,
        })?;

    if !status.success() {
        return Err(WattWardenError::Config("systemctl enable failed".into()));
    }

    Ok(())
}

pub fn uninstall_systemd_service() -> Result<()> {
    let _ = Command::new("systemctl").args(["stop", "wattwarden"]).status();
    let _ = Command::new("systemctl").args(["disable", "wattwarden"]).status();

    if Path::new(SYSTEMD_UNIT_PATH).exists() {
        let _ = fs::remove_file(SYSTEMD_UNIT_PATH);
        let _ = Command::new("systemctl").arg("daemon-reload").status();
    }

    Ok(())
}
