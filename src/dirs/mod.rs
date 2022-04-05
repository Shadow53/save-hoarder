//! Functions to determine special folders for Hoard to work with on different platforms.
use std::path::{PathBuf, Path};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::{get_known_folder, set_known_folder};
#[cfg(windows)]
pub use windows::Win32::UI::Shell::{FOLDERID_Profile, FOLDERID_RoamingAppData};

#[cfg(unix)]
use unix as sys;
#[cfg(windows)]
use win as sys;

/// The TLD portion of the application identifier.
pub const TLD: &str = "com";
/// The Company portion of the application identifier.
pub const COMPANY: &str = "shadow53";
/// The Project Name portion of the application identifier.
pub const PROJECT: &str = "hoard";
/// The environment variable that takes precendence over data dir detection.
pub const DATA_DIR_ENV: &str = "HOARD_DATA_DIR";
/// The environment variable that takes precendence over config dir detection.
pub const CONFIG_DIR_ENV: &str = "HOARD_CONFIG_DIR";

#[inline]
fn path_from_env(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

/// Returns the current user's home directory.
///
/// - Windows: The "known folder" `FOLDERID_Profile`, fallback to `%USERPROFILE%`.
/// - macOS/Linux/BSD: The value of `$HOME`.
#[must_use]
#[inline]
pub fn home_dir() -> PathBuf {
    sys::home_dir()
}

/// Returns Hoard's configuration directory for the current user.
///
/// Returns the contents of `HOARD_CONFIG_DIR`, if set, otherwise:
///
/// - Windows: `{appdata}/shadow53/hoard/config` where `{appdata}` is the "known folder"
///   `FOLDERID_RoamingAppData` or the value of `%APPDATA%`.
/// - macOS: `${XDG_CONFIG_HOME}/hoard`, if `XDG_CONFIG_HOME` is set, otherwise
///   `$HOME/Library/Application Support/com.shadow53.hoard`.
/// - Linux/BSD: `${XFG_CONFIG_HOME}/hoard`, if `XDG_CONFIG_HOME` is set, otherwise `$HOME/.config/hoard`.
#[must_use]
#[inline]
pub fn config_dir() -> PathBuf {
    path_from_env(CONFIG_DIR_ENV).unwrap_or_else(sys::config_dir)
}

/// Returns Hoard's data directory for the current user.
///
/// - Windows: `{appdata}/shadow53/hoard/data` where `{appdata}` is the "known folder"
///   `FOLDERID_RoamingAppData` or the value of `%APPDATA%`.
/// - macOS: `${XDG_DATA_HOME}/hoard`, if `XDG_DATA_HOME` is set, otherwise
///   `$HOME/Library/Application Support/com.shadow53.hoard`.
/// - Linux/BSD: `${XFG_DATA_HOME}/hoard`, if `XDG_DATA_HOME` is set, otherwise `$HOME/.local/share/hoard`.
#[must_use]
#[inline]
pub fn data_dir() -> PathBuf {
    path_from_env(DATA_DIR_ENV).unwrap_or_else(sys::data_dir)
}

/// Set the environment variable that overrides Hoard's config directory.
///
/// See [`CONFIG_DIR_ENV`].
pub fn set_config_dir(path: &Path) {
    std::env::set_var(CONFIG_DIR_ENV, path);
}

/// Set the environment variable that overrides Hoard's data directory.
///
/// See [`DATA_DIR_ENV`].
pub fn set_data_dir(path: &Path) {
    std::env::set_var(DATA_DIR_ENV, path);
}
