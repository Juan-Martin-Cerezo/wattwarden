//! Root privilege detection across Unix environments (Linux and macOS).

pub fn is_root() -> bool {
    nix::unistd::Uid::effective().is_root()
}
