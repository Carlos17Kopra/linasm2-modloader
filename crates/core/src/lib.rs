pub mod atomic;
pub mod branding;
pub mod error;
pub mod i18n;
pub mod import;
pub mod launch;
pub mod library;
pub mod pak_config;
pub mod paths;
pub mod platform;
pub mod profile;
pub mod saves;
pub mod settings;

pub use branding::{APP_NAME, APP_NAME_SHORT, APP_SLUG, APP_SUBTITLE, LEGACY_APP_SLUG};
pub use error::{Error, Result};

/// Steam AppID of Warhammer 40,000: Space Marine 2.
pub const APP_ID: u32 = 2183900;
