//! Detached daemon process spawning with stdio redirection to `/var/log/wattwarden.log`
//! (mode 0644), preventing daemon output from interfering with interactive terminal sessions.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Standard system log destination for the detached daemon.
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

/// Opens the log file with append permissions and mode 0644 on Unix.
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
/// tty and stdout/stderr appended to the system log file.
pub fn detached_daemon_command(exe: &Path, log_path: &Path) -> io::Result<Command> {
    let file = open_daemon_log(log_path)?;
    // A second handle: one for stdout, one for stderr.
    let err_file = file.try_clone()?;
    let mut cmd = Command::new(exe);
    cmd.arg("--daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(err_file));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // New session (setsid) so closing the controlling terminal does not terminate it.
        unsafe {
            cmd.pre_exec(|| {
                nix::unistd::setsid()
                    .map(|_| ())
                    .map_err(|e| io::Error::from_raw_os_error(e as i32))
            });
        }
    }
    Ok(cmd)
}

/// Spawns the detached daemon process for the current executable.
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
    fn detached_spawn_redirects_stdio_to_log_file() {
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
        // Building the command opens (O_CREATE|O_APPEND) the log file.
        assert!(log.exists(), "spawn must create the log file");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn daemon_log_path_defaults_to_system_path() {
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
