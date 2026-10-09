//! Service installation and system binary synchronization.

#[cfg(unix)]
use std::path::Path;
use std::process::Command;

/// Target system install location for the WattWarden binary.
pub const INSTALLED_BINARY: &str = "/usr/local/bin/wattwarden";

#[cfg(target_os = "linux")]
const SYSTEMD_UNIT_PATH: &str = "/etc/systemd/system/wattwarden.service";

#[cfg(target_os = "macos")]
const PLIST_PATH: &str = "/Library/LaunchDaemons/com.wattwarden.daemon.plist";

/// Synchronizes the executing binary to `/usr/local/bin/wattwarden` when running as root.
pub fn sync_installed_binary() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if nix::unistd::Uid::effective().as_raw() != 0 {
            return;
        }
        let Ok(current_exe) = std::env::current_exe() else {
            return;
        };
        if current_exe == Path::new(INSTALLED_BINARY) {
            return;
        }
        let Ok(bytes) = std::fs::read(&current_exe) else {
            return;
        };
        let _ = std::fs::create_dir_all("/usr/local/bin");
        if std::fs::write(INSTALLED_BINARY, bytes).is_ok() {
            let _ =
                std::fs::set_permissions(INSTALLED_BINARY, std::fs::Permissions::from_mode(0o755));
        }
    }
}

/// Systemd unit definition for the WattWarden background service.
#[cfg(target_os = "linux")]
pub fn systemd_unit_text() -> String {
    format!(
        "[Unit]\n\
         Description=WattWarden Auto Power and Hardware Management Daemon\n\
         After=multi-user.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={INSTALLED_BINARY} --daemon\n\
         Restart=always\n\
         RestartSec=3\n\
         KillMode=process\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n"
    )
}

/// Installs and activates the platform background service.
pub fn install_service() -> Result<(), String> {
    sync_installed_binary();

    #[cfg(target_os = "linux")]
    {
        let content = systemd_unit_text();

        std::fs::write(SYSTEMD_UNIT_PATH, content).map_err(|e| e.to_string())?;
        let _ = Command::new("systemctl").arg("daemon-reload").status();
        let _ = Command::new("systemctl")
            .args(["enable", "--now", "wattwarden.service"])
            .status();
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let content = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
             \x20   <key>Label</key>\n\
             \x20   <string>com.wattwarden.daemon</string>\n\
             \x20   <key>ProgramArguments</key>\n\
             \x20   <array>\n\
             \x20       <string>{INSTALLED_BINARY}</string>\n\
             \x20       <string>--daemon</string>\n\
             \x20   </array>\n\
             \x20   <key>RunAtLoad</key>\n\
             \x20   <true/>\n\
             \x20   <key>KeepAlive</key>\n\
             \x20   <true/>\n\
             \x20   <key>StandardErrorPath</key>\n\
             \x20   <string>/var/log/wattwarden.log</string>\n\
             \x20   <key>StandardOutPath</key>\n\
             \x20   <string>/var/log/wattwarden.log</string>\n\
             </dict>\n\
             </plist>\n"
        );
        std::fs::write(PLIST_PATH, content).map_err(|e| e.to_string())?;
        let _ = Command::new("launchctl")
            .args(["load", "-w", PLIST_PATH])
            .status();
        return Ok(());
    }

    #[allow(unreachable_code)]
    Ok(())
}

/// Uninstalls and removes the platform background service.
pub fn uninstall_service() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("systemctl")
            .args(["disable", "--now", "wattwarden.service"])
            .status();
        let _ = std::fs::remove_file(SYSTEMD_UNIT_PATH);
        let _ = Command::new("systemctl").arg("daemon-reload").status();
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("launchctl")
            .args(["unload", "-w", PLIST_PATH])
            .status();
        let _ = std::fs::remove_file(PLIST_PATH);
        return Ok(());
    }

    #[allow(unreachable_code)]
    Ok(())
}

/// Locks down the systemd unit text, so it only exists where that unit does.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// The unit text is part of the contract (`PARITY.md` §3): lock it down exactly,
    /// including `Restart=always`, `RestartSec=3`, `KillMode=process` and the
    /// `--daemon` argument.
    #[cfg(target_os = "linux")]
    #[test]
    fn systemd_unit_text_matches_parity_contract() {
        let expected = "[Unit]\n\
Description=WattWarden Auto Power and Hardware Management Daemon\n\
After=multi-user.target\n\
\n\
[Service]\n\
Type=simple\n\
ExecStart=/usr/local/bin/wattwarden --daemon\n\
Restart=always\n\
RestartSec=3\n\
KillMode=process\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n";

        assert_eq!(systemd_unit_text(), expected);
    }
}
