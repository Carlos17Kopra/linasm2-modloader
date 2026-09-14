pub mod atomic;
pub mod error;
pub mod pak_config;
pub mod paths;
pub mod platform;

pub use error::{Error, Result};

/// Steam-AppID von Warhammer 40.000: Space Marine 2.
pub const APP_ID: u32 = 2183900;
