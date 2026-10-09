use crate::sysfs::SysfsRoot;
use serde::Deserialize;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;
use wattwarden_core::CompositorFocus;

#[derive(Debug, Deserialize)]
struct HyprActiveWindow {
    #[serde(default)]
    class: String,
    #[serde(default)]
    #[allow(dead_code)]
    title: String,
}

pub struct HyprlandIpc {
    cmd_socket_path: Option<PathBuf>,
    root: SysfsRoot,
}

impl HyprlandIpc {
    pub fn new() -> Self {
        Self::with_root(SysfsRoot::from_env())
    }

    /// Builds the compositor bridge against a relocated root (used by tests and by
    /// anything that wants the socket scan to stay inside `WATTWARDEN_SYSFS_ROOT`).
    pub fn with_root(root: SysfsRoot) -> Self {
        let cmd_socket_path = Self::discover_socket_with_root(".socket.sock", &root);
        Self {
            cmd_socket_path,
            root,
        }
    }

    pub fn discover_event_socket() -> Option<PathBuf> {
        Self::discover_socket_with_root(".socket2.sock", &SysfsRoot::from_env())
    }

    /// Root this bridge was built against (diagnostics/tests).
    pub fn root(&self) -> &SysfsRoot {
        &self.root
    }

    fn discover_socket_with_root(socket_name: &str, root: &SysfsRoot) -> Option<PathBuf> {
        // 1. Direct environment variable lookup
        if let (Ok(runtime_dir), Ok(sig)) = (
            std::env::var("XDG_RUNTIME_DIR"),
            std::env::var("HYPRLAND_INSTANCE_SIGNATURE"),
        ) {
            let p = PathBuf::from(runtime_dir)
                .join("hypr")
                .join(sig)
                .join(socket_name);
            if p.exists() {
                return Some(p);
            }
        }

        // 2. Multi-user scan under /run/user/ (handles sudo/daemon root execution)
        let run_user = root.path("run/user");
        if let Ok(user_entries) = fs::read_dir(&run_user) {
            for user_entry in user_entries.flatten() {
                let hypr_dir = user_entry.path().join("hypr");
                if let Ok(hypr_entries) = fs::read_dir(hypr_dir) {
                    for inst in hypr_entries.flatten() {
                        let candidate = inst.path().join(socket_name);
                        if candidate.exists() {
                            return Some(candidate);
                        }
                    }
                }
            }
        }
        None
    }
}

impl Default for HyprlandIpc {
    fn default() -> Self {
        Self::new()
    }
}

impl CompositorFocus for HyprlandIpc {
    fn active_window_class(&self) -> Option<String> {
        let path = self.cmd_socket_path.as_ref()?;
        let mut stream = UnixStream::connect(path).ok()?;
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .ok()?;
        stream
            .set_write_timeout(Some(Duration::from_millis(50)))
            .ok()?;

        // Send JSON request for activewindow to Hyprland socket
        stream.write_all(b"j/activewindow").ok()?;

        let mut buffer = String::new();
        stream.read_to_string(&mut buffer).ok()?;

        if let Ok(win) = serde_json::from_str::<HyprActiveWindow>(&buffer) {
            if !win.class.is_empty() {
                return Some(win.class.to_lowercase());
            }
        }
        None
    }
}

impl HyprlandIpc {
    pub fn with_socket(cmd_socket_path: Option<PathBuf>) -> Self {
        Self {
            cmd_socket_path,
            root: SysfsRoot::from_env(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hypr_active_window_deserialization() {
        let json = r#"{"class":"kitty","title":"alacritty terminal"}"#;
        let win: HyprActiveWindow = serde_json::from_str(json).unwrap();
        assert_eq!(win.class, "kitty");
        assert_eq!(win.title, "alacritty terminal");

        let empty_json = r#"{}"#;
        let empty_win: HyprActiveWindow = serde_json::from_str(empty_json).unwrap();
        assert!(empty_win.class.is_empty());
    }

    #[test]
    fn test_compositor_none_fallback() {
        let ipc = HyprlandIpc::with_socket(None);
        assert_eq!(ipc.active_window_class(), None);
    }
}
