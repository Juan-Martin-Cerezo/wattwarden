//! Central sysfs/procfs access layer.
//!
//! Every read or write performed by the Linux backend goes through [`SysfsRoot`].
//! The root defaults to `/` (the real `/sys` and `/proc`) but can be relocated with
//! the `WATTWARDEN_SYSFS_ROOT` environment variable so unit tests can point the whole
//! backend at a fabricated directory tree built with `std::env::temp_dir()`.
//!
//! The Go reference implementation (`hal/backend_linux.go`) uses `readSys()` which
//! returns `""` when a path is missing or unreadable, and `writeSys()` which ignores
//! errors. [`SysfsRoot::read`] and [`SysfsRoot::write_best_effort`] reproduce those
//! semantics exactly so the fallbacks documented in `PARITY.md` §2 keep working.

use std::fs;
use std::path::{Path, PathBuf};
use wattwarden_core::{Result, WattWardenError};

/// Environment variable used to relocate the sysfs/procfs root (default `/`).
pub const SYSFS_ROOT_ENV: &str = "WATTWARDEN_SYSFS_ROOT";

/// A relocated view of `/sys` (and `/proc`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysfsRoot {
    root: PathBuf,
}

impl Default for SysfsRoot {
    fn default() -> Self {
        Self::from_env()
    }
}

impl SysfsRoot {
    /// Reads `WATTWARDEN_SYSFS_ROOT` (defaulting to `/`).
    pub fn from_env() -> Self {
        match std::env::var(SYSFS_ROOT_ENV) {
            Ok(v) if !v.trim().is_empty() => Self::new(v.trim()),
            _ => Self::new("/"),
        }
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolves a kernel-style path (`/sys/...`, `/proc/...`, leading slash optional)
    /// under this root.
    pub fn path(&self, rel: &str) -> PathBuf {
        let stripped = rel.trim_start_matches('/');
        if stripped.is_empty() {
            self.root.clone()
        } else {
            self.root.join(stripped)
        }
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.path(rel).exists()
    }

    /// Go `readSys()`: trimmed file content, or `""` when the path is missing.
    pub fn read(&self, rel: &str) -> String {
        self.read_path(&self.path(rel))
    }

    pub fn read_path(&self, path: &Path) -> String {
        fs::read_to_string(path)
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    }

    /// Rust-flavoured read that surfaces I/O errors instead of returning `""`.
    pub fn read_checked(&self, rel: &str) -> Result<String> {
        let p = self.path(rel);
        fs::read_to_string(&p)
            .map(|s| s.trim().to_string())
            .map_err(|e| WattWardenError::Io { path: p, source: e })
    }

    /// Go `strconv.Atoi(readSys(..))` with the `err != nil -> fallback` idiom:
    /// `None` means "missing or unparsable".
    pub fn read_u64(&self, rel: &str) -> Option<u64> {
        self.read(rel).parse::<u64>().ok()
    }

    pub fn read_i64(&self, rel: &str) -> Option<i64> {
        self.read(rel).parse::<i64>().ok()
    }

    pub fn read_f64(&self, rel: &str) -> Option<f64> {
        self.read(rel).parse::<f64>().ok()
    }

    /// Go `writeSys()` but surfacing errors to callers that care.
    pub fn write(&self, rel: &str, val: &str) -> Result<()> {
        let p = self.path(rel);
        fs::write(&p, val).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                WattWardenError::PermissionDenied(format!("Cannot write to {}", p.display()))
            } else {
                WattWardenError::Io { path: p, source: e }
            }
        })
    }

    /// Go `writeSys()` exactly: best effort, errors silently swallowed.
    pub fn write_best_effort(&self, rel: &str, val: &str) {
        let _ = fs::write(self.path(rel), val);
    }

    /// Absolute-path variant of [`SysfsRoot::write_best_effort`], for callers that
    /// already hold a resolved path.
    pub fn write_best_effort_path(&self, path: &Path, val: &str) {
        let _ = fs::write(path, val);
    }

    /// Sorted entry names of a directory (`[]` when unreadable).
    pub fn names(&self, rel: &str) -> Vec<String> {
        let mut names: Vec<String> = match fs::read_dir(self.path(rel)) {
            Ok(entries) => entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .collect(),
            Err(_) => Vec::new(),
        };
        names.sort();
        names
    }

    /// Sorted full entry paths of a directory (`[]` when unreadable).
    pub fn entries(&self, rel: &str) -> Vec<PathBuf> {
        self.names(rel)
            .into_iter()
            .map(|n| self.path(&join_rel(rel, &n)))
            .collect()
    }

    /// Go `filepath.Glob("<dir>/*<suffix>")` restricted to existing entries.
    pub fn glob(&self, dir: &str, matches: impl Fn(&str) -> bool) -> Vec<PathBuf> {
        self.names(dir)
            .into_iter()
            .filter(|n| matches(n))
            .map(|n| self.path(&join_rel(dir, &n)))
            .collect()
    }
}

fn join_rel(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

// ---------------------------------------------------------------------------
// Legacy absolute-path helpers (kept for backwards compatibility).
// ---------------------------------------------------------------------------

pub fn read_sysfs_string(path: impl AsRef<Path>) -> Result<String> {
    let p = path.as_ref();
    fs::read_to_string(p)
        .map(|s| s.trim().to_string())
        .map_err(|e| WattWardenError::Io {
            path: p.to_path_buf(),
            source: e,
        })
}

pub fn read_sysfs_u64(path: impl AsRef<Path>) -> Result<u64> {
    let p = path.as_ref();
    let val_str = read_sysfs_string(p)?;
    val_str
        .parse::<u64>()
        .map_err(|e| WattWardenError::ParseInt {
            path: p.to_path_buf(),
            val: val_str,
            source: e,
        })
}

pub fn read_sysfs_u32(path: impl AsRef<Path>) -> Result<u32> {
    let p = path.as_ref();
    let val_str = read_sysfs_string(p)?;
    val_str
        .parse::<u32>()
        .map_err(|e| WattWardenError::ParseInt {
            path: p.to_path_buf(),
            val: val_str,
            source: e,
        })
}

pub fn read_sysfs_i64(path: impl AsRef<Path>) -> Result<i64> {
    let p = path.as_ref();
    let val_str = read_sysfs_string(p)?;
    val_str
        .parse::<i64>()
        .map_err(|e| WattWardenError::ParseInt {
            path: p.to_path_buf(),
            val: val_str,
            source: e,
        })
}

pub fn write_sysfs_string(path: impl AsRef<Path>, val: &str) -> Result<()> {
    let p = path.as_ref();
    fs::write(p, val).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            WattWardenError::PermissionDenied(format!("Cannot write to {}", p.display()))
        } else {
            WattWardenError::Io {
                path: p.to_path_buf(),
                source: e,
            }
        }
    })
}

pub fn write_sysfs_u64(path: impl AsRef<Path>, val: u64) -> Result<()> {
    write_sysfs_string(path, &val.to_string())
}

/// Legacy glob helper (kept for API compatibility).
pub fn glob_dirs(pattern_prefix: &str, dir_suffix: &str) -> Vec<PathBuf> {
    let mut results = Vec::new();
    let parent = Path::new(pattern_prefix);
    if let Ok(entries) = fs::read_dir(parent) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if file_name.starts_with(dir_suffix) {
                    results.push(path);
                }
            }
        }
    }
    results.sort();
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_write_sysfs() {
        let dir = std::env::temp_dir().join(format!("wattwarden_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let test_file = dir.join("test_val");

        write_sysfs_string(&test_file, "42000").unwrap();
        assert_eq!(read_sysfs_string(&test_file).unwrap(), "42000");
        assert_eq!(read_sysfs_u64(&test_file).unwrap(), 42000);
        assert_eq!(read_sysfs_u32(&test_file).unwrap(), 42000);
        assert_eq!(read_sysfs_i64(&test_file).unwrap(), 42000);

        // ParseInt error handling
        write_sysfs_string(&test_file, "not_a_number").unwrap();
        assert!(read_sysfs_u64(&test_file).is_err());
        assert!(read_sysfs_u32(&test_file).is_err());
        assert!(read_sysfs_i64(&test_file).is_err());

        // Missing file error handling
        let missing = dir.join("non_existent_file");
        assert!(read_sysfs_string(&missing).is_err());

        let _ = fs::remove_file(test_file);
        let _ = fs::remove_dir(dir);
    }

    #[test]
    fn test_glob_dirs() {
        let dir = std::env::temp_dir().join(format!("ww_glob_test_{}", std::process::id()));
        let _ = fs::create_dir_all(dir.join("prefix_alpha"));
        let _ = fs::create_dir_all(dir.join("prefix_beta"));
        let _ = fs::create_dir_all(dir.join("other_gamma"));

        let found = glob_dirs(dir.to_str().unwrap(), "prefix_");
        assert_eq!(found.len(), 2);
        assert!(found[0].to_str().unwrap().contains("prefix_alpha"));
        assert!(found[1].to_str().unwrap().contains("prefix_beta"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_sysfs_root_resolution() {
        let root = SysfsRoot::new("/tmp/ww-fake-root");
        assert_eq!(
            root.path("/sys/class/backlight"),
            PathBuf::from("/tmp/ww-fake-root/sys/class/backlight")
        );
        // Leading slash is optional and must not escape the root.
        assert_eq!(
            root.path("proc/sys/vm/drop_caches"),
            PathBuf::from("/tmp/ww-fake-root/proc/sys/vm/drop_caches")
        );
        assert_eq!(root.path("/"), PathBuf::from("/tmp/ww-fake-root"));

        // Go readSys semantics: missing files read as empty, no panic.
        assert_eq!(root.read("/sys/nope/nope"), "");
        assert_eq!(root.read_u64("/sys/nope/nope"), None);
    }

    #[test]
    fn test_sysfs_root_glob_is_sorted_and_suffix_filtered() {
        let base = std::env::temp_dir().join(format!("ww_root_glob_{}", std::process::id()));
        let _ = fs::create_dir_all(base.join("class/leds/dell::kbd_backlight"));
        let _ = fs::create_dir_all(base.join("class/leds/input3::capslock"));
        let _ = fs::create_dir_all(base.join("class/leds/tpacpi::kbd_backlight"));

        let root = SysfsRoot::new(&base);
        let leds = root.glob("class/leds", |n| n.ends_with("kbd_backlight"));
        assert_eq!(leds.len(), 2);
        assert!(leds[0].ends_with("class/leds/dell::kbd_backlight"));
        assert!(leds[1].ends_with("class/leds/tpacpi::kbd_backlight"));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn test_write_best_effort_never_panics() {
        let root = SysfsRoot::new("/definitely/not/a/real/root");
        root.write_best_effort("/proc/sys/vm/drop_caches", "3");
        assert!(root.write("/proc/sys/vm/drop_caches", "3").is_err());
    }
}
