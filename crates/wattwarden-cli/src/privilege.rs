use nix::unistd::Uid;

pub fn is_root() -> bool {
    Uid::effective().is_root()
}

pub fn require_root() -> Result<(), String> {
    if !is_root() {
        Err("Administrator/root privileges are required to modify system hardware settings.\nPlease run: sudo wattwarden".into())
    } else {
        Ok(())
    }
}
