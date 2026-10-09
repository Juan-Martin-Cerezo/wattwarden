// Shared, platform-agnostic plumbing. Compiled everywhere on purpose: the
// macOS backend is a pure CLI wrapper, so keeping its command/parse logic out of
// any `cfg` is what lets tests run on Linux against fake binaries on `PATH`.
pub mod exec;
pub mod parse;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "linux")]
pub type PlatformBackend = linux::LinuxBackend;

// The macOS backend is compiled on every host; only the `PlatformBackend`
// selection and the glob re-export stay platform-gated.
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;
#[cfg(target_os = "macos")]
pub type PlatformBackend = macos::MacOsBackend;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub type PlatformBackend = fallback::FallbackBackend;

// Always available for cross-platform testing and non-destructive fallbacks
pub mod fallback;

// Backwards-compatibility aliases ensuring zero breaking changes across all crates
pub type SystemBackend = PlatformBackend;
pub type UniversalBackend = PlatformBackend;

#[cfg(not(target_os = "linux"))]
pub type LinuxBackend = PlatformBackend;
