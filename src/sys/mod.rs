//! Operating-system glue: the only code that talks to real hardware, kept thin
//! because the tests cannot reach it.

use std::path::PathBuf;

use anyhow::Result;

mod hid;
pub use hid::HidApi;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{Desktop, config_dir};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{Desktop, config_dir};

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
compile_error!("edgehop supports only macOS and Windows");

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("edgehop").join("config.toml"))
}
