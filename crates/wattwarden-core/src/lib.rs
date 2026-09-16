pub mod config;
pub mod error;
pub mod traits;
pub mod typestate;

pub use config::{Config, PowerProfile};
pub use error::{Result, WattWardenError};
pub use traits::*;
pub use typestate::{DeviceGovernor, Extreme, Normal, Performance};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_from_str() {
        assert_eq!("performance".parse::<PowerProfile>().unwrap(), PowerProfile::Performance);
        assert_eq!("extreme".parse::<PowerProfile>().unwrap(), PowerProfile::Extreme);
        assert_eq!("normal".parse::<PowerProfile>().unwrap(), PowerProfile::Normal);
        assert_eq!("auto".parse::<PowerProfile>().unwrap(), PowerProfile::AutoExtreme);
        assert!("invalid".parse::<PowerProfile>().is_err());
    }

    #[test]
    fn test_typestate_transitions() {
        let governor = DeviceGovernor::new();
        let perf = governor.to_performance();
        let extreme = perf.to_extreme();
        let normal = extreme.to_normal();
        let _ = normal.to_extreme();
    }
}
