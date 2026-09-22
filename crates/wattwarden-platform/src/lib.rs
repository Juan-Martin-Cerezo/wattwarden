// Shared, platform-agnostic plumbing. Compiled everywhere on purpose: the
// macOS/Windows backends are pure CLI wrappers (plus one Win32 call), so keeping
// their command/parse logic out of any `cfg` is what lets the Go-parity tests run
// on this Linux host against fake binaries on `PATH`.
pub mod exec;
pub mod parse;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "linux")]
pub type PlatformBackend = linux::LinuxBackend;

// The macOS/Windows backends are compiled on every host; only the `PlatformBackend`
// selection and the glob re-export stay platform-gated.
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;
#[cfg(target_os = "macos")]
pub type PlatformBackend = macos::MacOsBackend;

pub mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;
#[cfg(target_os = "windows")]
pub type PlatformBackend = windows::WindowsBackend;

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub type PlatformBackend = fallback::FallbackBackend;

// Always available for cross-platform testing and non-destructive fallbacks
pub mod fallback;

// Backwards-compatibility aliases ensuring zero breaking changes across all crates
pub type SystemBackend = PlatformBackend;
pub type UniversalBackend = PlatformBackend;

#[cfg(not(target_os = "linux"))]
pub type LinuxBackend = PlatformBackend;
