//! Root privilege detection. Go's `hasPrivileges()` is `os.Geteuid() == 0` on Unix.

#[cfg(unix)]
pub fn is_root() -> bool {
    nix::unistd::Uid::effective().is_root()
}

#[cfg(not(unix))]
pub fn is_root() -> bool {
    true
}
