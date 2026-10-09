//! Go `service.SpawnDetachedDaemon` (`service/spawn_unix.go`): spawn the own
//! executable with `--daemon`, detached from the controlling terminal, with
//! stdout/stderr appended to `/var/log/wattwarden.log` (mode 0644, no rotation)
//! so daemon logs never land on the TUI's alternate screen.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Go log destination for the detached daemon (`spawn_unix.go`, macOS plist).
pub const DAEMON_LOG_PATH: &str = "/var/log/wattwarden.log";

/// Resolves the log file for a detached spawn. Production uses
/// [`DAEMON_LOG_PATH`]; tests inject a tempdir file via `WATTWARDEN_DAEMON_LOG`.
pub fn daemon_log_path() -> PathBuf {
    if let Ok(p) = std::env::var("WATTWARDEN_DAEMON_LOG") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from(DAEMON_LOG_PATH)
}

/// Opens the log file exactly like Go's
/// `os.OpenFile(path, O_CREATE|O_WRONLY|O_APPEND, 0644)`.
pub fn open_daemon_log(path: &Path) -> io::Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644));
    }
    Ok(file)
}

/// Builds the detached `--daemon` command for `exe`: stdin detached from the
/// tty and stdout/stderr appended to the Go log file, like
/// `service/spawn_unix.go`. Returns the log path so tests can assert on it.
pub fn detached_daemon_command(exe: &Path, log_path: &Path) -> io::Result<Command> {
    let file = open_daemon_log(log_path)?;
    // A second handle: one for stdout, one for stderr (Go assigns both).
    let err_file = file.try_clone()?;
    let mut cmd = Command::new(exe);
    cmd.arg("--daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(err_file));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Go `Setsid: true`: new session so closing the terminal does not kill it.
        unsafe {
            cmd.pre_exec(|| {
                nix::unistd::setsid()
                    .map(|_| ())
                    .map_err(|e| io::Error::from_raw_os_error(e as i32))
            });
        }
    }
    #[cfg(windows)]
    {
        // Go `spawn_windows.go`: CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW.
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    Ok(cmd)
}

/// Go `SpawnDetachedDaemon`: build the detached command for the current
/// executable and start it.
pub fn spawn_detached_daemon() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let log_path = daemon_log_path();
    detached_daemon_command(&exe, &log_path)?.spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detached_spawn_redirects_stdio_to_go_log_file() {
        let dir = std::env::temp_dir().join(format!("ww_spawn_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("wattwarden.log");

        let exe = std::env::current_exe().unwrap();
        let cmd = detached_daemon_command(&exe, &log).unwrap();
        // The binary path plus the single --daemon argument.
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(cmd.get_program(), exe.as_os_str());
        assert_eq!(args, ["--daemon"]);
        // Building the command opens (O_CREATE|O_APPEND) the Go log file.
        assert!(log.exists(), "spawn must create the Go log file");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn daemon_log_path_defaults_to_go_path() {
        std::env::remove_var("WATTWARDEN_DAEMON_LOG");
        assert_eq!(daemon_log_path(), PathBuf::from(DAEMON_LOG_PATH));
    }

    #[test]
    fn daemon_log_path_override_is_honored() {
        std::env::set_var("WATTWARDEN_DAEMON_LOG", "/tmp/ww-test-override.log");
        assert_eq!(
            daemon_log_path(),
            PathBuf::from("/tmp/ww-test-override.log")
        );
        std::env::remove_var("WATTWARDEN_DAEMON_LOG");
    }
}
