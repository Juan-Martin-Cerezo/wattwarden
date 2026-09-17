use crate::config::PowerProfile;
use std::marker::PhantomData;

/// Sealed marker traits for power profile typestates
pub trait ProfileState: Send + Sync {
    const PROFILE: PowerProfile;
}

#[derive(Debug, Clone, Copy)]
pub struct Normal;
impl ProfileState for Normal {
    const PROFILE: PowerProfile = PowerProfile::Normal;
}

#[derive(Debug, Clone, Copy)]
pub struct Performance;
impl ProfileState for Performance {
    const PROFILE: PowerProfile = PowerProfile::Performance;
}

#[derive(Debug, Clone, Copy)]
pub struct Extreme;
impl ProfileState for Extreme {
    const PROFILE: PowerProfile = PowerProfile::Extreme;
}

/// Device wrapper enforcing typestate transitions
pub struct DeviceGovernor<S: ProfileState> {
    _state: PhantomData<S>,
}

impl DeviceGovernor<Normal> {
    pub fn new() -> Self {
        Self {
            _state: PhantomData,
        }
    }

    pub fn to_performance(self) -> DeviceGovernor<Performance> {
        DeviceGovernor {
            _state: PhantomData,
        }
    }

    pub fn to_extreme(self) -> DeviceGovernor<Extreme> {
        DeviceGovernor {
            _state: PhantomData,
        }
    }
}

impl DeviceGovernor<Performance> {
    pub fn to_normal(self) -> DeviceGovernor<Normal> {
        DeviceGovernor {
            _state: PhantomData,
        }
    }

    pub fn to_extreme(self) -> DeviceGovernor<Extreme> {
        DeviceGovernor {
            _state: PhantomData,
        }
    }
}

impl DeviceGovernor<Extreme> {
    pub fn to_normal(self) -> DeviceGovernor<Normal> {
        DeviceGovernor {
            _state: PhantomData,
        }
    }

    pub fn to_performance(self) -> DeviceGovernor<Performance> {
        DeviceGovernor {
            _state: PhantomData,
        }
    }
}

impl Default for DeviceGovernor<Normal> {
    fn default() -> Self {
        Self::new()
    }
}
