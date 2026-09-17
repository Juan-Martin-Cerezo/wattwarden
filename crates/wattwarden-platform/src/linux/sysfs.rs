use std::fs;
use std::path::{Path, PathBuf};
use wattwarden_core::{Result, WattWardenError};

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
}
