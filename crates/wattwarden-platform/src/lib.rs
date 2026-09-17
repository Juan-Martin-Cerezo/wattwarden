#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "linux")]
pub type PlatformBackend = linux::LinuxBackend;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;
#[cfg(target_os = "macos")]
pub type PlatformBackend = macos::MacOsBackend;

#[cfg(target_os = "windows")]
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
