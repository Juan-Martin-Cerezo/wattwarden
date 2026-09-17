#[cfg(unix)]
pub fn is_root() -> bool {
    nix::unistd::Uid::effective().is_root()
}

#[cfg(not(unix))]
pub fn is_root() -> bool {
    true
}

pub fn require_root() -> Result<(), String> {
    if !is_root() {
        Err("Administrator/root privileges are required to modify system hardware settings.\nPlease run: sudo wattwarden".into())
    } else {
        Ok(())
    }
}
