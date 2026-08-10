//! Where config and skins live on each platform.
//!
//! Isolated from the rest of config so the OS lookup happens in exactly one
//! place and can be overridden by callers that need to.

use directories::ProjectDirs;
use std::path::PathBuf;

const QUALIFIER: &str = "com";
const ORGANISATION: &str = "batates";
const APPLICATION: &str = "batates";

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from(QUALIFIER, ORGANISATION, APPLICATION)
}

/// The config file location.
///
/// macOS:   `~/Library/Application Support/com.batates.batates/config.toml`
/// Linux:   `~/.config/batates/config.toml`
/// Windows: `%APPDATA%\batates\batates\config\config.toml`
///
/// Falls back to the current directory when the OS reports no home, which is
/// rare but real in stripped service environments.
pub fn default_config_path() -> PathBuf {
    match project_dirs() {
        Some(dirs) => dirs.config_dir().join("config.toml"),
        None => PathBuf::from("batates.toml"),
    }
}

/// The directory holding user-installed skins, one subdirectory per skin.
///
/// A repo-local `assets/skins` wins when present so a development checkout
/// works without installing anything.
pub fn skins_dir() -> PathBuf {
    let local = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/skins");
    if local.is_dir() {
        return local;
    }
    match project_dirs() {
        Some(dirs) => dirs.data_dir().join("skins"),
        None => PathBuf::from("skins"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_is_absolute_and_named() {
        let path = default_config_path();
        assert!(path.ends_with("config.toml"), "{path:?}");
    }

    #[test]
    fn skins_dir_finds_the_repo_copy_in_a_checkout() {
        let dir = skins_dir();
        assert!(
            dir.join("panda").is_dir(),
            "{dir:?} should hold the panda skin"
        );
    }
}
