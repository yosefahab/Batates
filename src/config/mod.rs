//! User configuration.
//!
//! TOML because this file is hand-edited and comments matter, unlike the skin
//! manifest which is tool-generated.
//!
//! Two types: [`RawConfig`] mirrors the file and is all-optional, [`Config`] is
//! validated and concrete. Nothing downstream ever sees an unchecked number.

pub mod paths;

use bevy::prelude::*;
use serde::Deserialize;
use std::num::NonZeroU8;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

use crate::core::input::GestureConfig;
use crate::core::rng::Seed;
use crate::skin::SkinSource;

/// The skin that ships in the binary.
pub const BUILTIN_SKIN: &str = "koala";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not read config at {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse config at {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: Box<toml::de::Error>,
    },
    #[error("pets must be between 1 and 64, got {got}")]
    PetCount { got: u32 },
    #[error("scale must be greater than 0 and at most 16, got {got}")]
    Scale { got: f32 },
    #[error("{field} must be greater than zero")]
    NotPositive { field: &'static str },
    #[error("skin name must not be empty or a path")]
    SkinName,
}

/// How many pets to spawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PetCount(pub NonZeroU8);

/// Sprite scale multiplier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PetScale(pub f32);

/// The file as written on disk. Every field optional so a partial config works.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawConfig {
    #[serde(default)]
    pub app: RawApp,
    #[serde(default)]
    pub behavior: RawBehavior,
    #[serde(default)]
    pub debug: RawDebug,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDebug {
    pub overlay: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawApp {
    pub skin: Option<String>,
    pub pets: Option<u32>,
    pub scale: Option<f32>,
    pub seed: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBehavior {
    pub click_to_summon: Option<bool>,
    pub double_click_ms: Option<u64>,
    pub drag_threshold_ms: Option<u64>,
}

/// Validated configuration.
#[derive(Resource, Debug, Clone)]
pub struct Config {
    pub skin: String,
    pub pets: PetCount,
    pub scale: PetScale,
    pub seed: Seed,
    pub click_to_summon: bool,
    pub gestures: GestureConfig,
    /// Draws each pet's hitbox and the cursor the app believes in.
    pub debug_overlay: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            skin: BUILTIN_SKIN.to_string(),
            pets: PetCount(NonZeroU8::new(1).expect("1 is non-zero")),
            scale: PetScale(1.5),
            seed: Seed(0),
            click_to_summon: true,
            gestures: GestureConfig::default(),
            debug_overlay: false,
        }
    }
}

impl TryFrom<RawConfig> for Config {
    type Error = ConfigError;

    fn try_from(raw: RawConfig) -> Result<Self, Self::Error> {
        let mut config = Config::default();

        if let Some(skin) = raw.app.skin {
            // A skin is a name inside the skins directory, never a path: this
            // is what stops a config escaping that directory.
            if skin.is_empty() || skin.contains(['/', '\\']) || skin.contains("..") {
                return Err(ConfigError::SkinName);
            }
            config.skin = skin;
        }

        if let Some(pets) = raw.app.pets {
            let count = u8::try_from(pets)
                .ok()
                .and_then(NonZeroU8::new)
                .filter(|n| n.get() <= 64)
                .ok_or(ConfigError::PetCount { got: pets })?;
            config.pets = PetCount(count);
        }

        if let Some(scale) = raw.app.scale {
            if !(scale.is_finite() && scale > 0.0 && scale <= 16.0) {
                return Err(ConfigError::Scale { got: scale });
            }
            config.scale = PetScale(scale);
        }

        if let Some(seed) = raw.app.seed {
            config.seed = Seed(seed);
        }

        if let Some(click_to_summon) = raw.behavior.click_to_summon {
            config.click_to_summon = click_to_summon;
        }

        if let Some(overlay) = raw.debug.overlay {
            config.debug_overlay = overlay;
        }

        config.gestures.double_click =
            positive_millis(raw.behavior.double_click_ms, "double_click_ms")?
                .unwrap_or(config.gestures.double_click);
        config.gestures.drag_threshold =
            positive_millis(raw.behavior.drag_threshold_ms, "drag_threshold_ms")?
                .unwrap_or(config.gestures.drag_threshold);

        Ok(config)
    }
}

fn positive_millis(
    value: Option<u64>,
    field: &'static str,
) -> Result<Option<Duration>, ConfigError> {
    match value {
        None => Ok(None),
        Some(0) => Err(ConfigError::NotPositive { field }),
        Some(ms) => Ok(Some(Duration::from_millis(ms))),
    }
}

impl Config {
    /// Where to load this config's skin from.
    ///
    /// A user skin directory wins; otherwise the built-in name resolves to the
    /// embedded skin. An unknown name still resolves to a directory so the
    /// loader reports a real "not found" rather than silently substituting.
    pub fn skin_source(&self, skins_dir: &Path) -> SkinSource {
        let dir = skins_dir.join(&self.skin);
        if dir.join("skin.ron").is_file() {
            return SkinSource::Directory(dir);
        }
        if self.skin == BUILTIN_SKIN {
            return SkinSource::Builtin;
        }
        SkinSource::Directory(dir)
    }
}

/// Reads the config file if it is there.
///
/// `Ok(None)` means the file is absent, which is expected and yields defaults.
/// `Err` means it exists but is unusable: a typo should be reported, not
/// silently ignored, so the caller is expected to fail loudly.
pub fn load_config(path: &Path) -> Result<Option<Config>, ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ConfigError::Read {
                path: path.display().to_string(),
                source,
            });
        }
    };

    let raw: RawConfig = toml::from_str(&text).map_err(|source| ConfigError::Parse {
        path: path.display().to_string(),
        source: Box::new(source),
    })?;

    Config::try_from(raw).map(Some)
}

/// Resolves the config path: `$BATATES_CONFIG` wins, else the platform default.
///
/// Env is read here, at the edge, so nothing deeper depends on ambient state.
pub fn config_path() -> PathBuf {
    match std::env::var_os("BATATES_CONFIG") {
        Some(path) => PathBuf::from(path),
        None => paths::default_config_path(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Config, ConfigError> {
        let raw: RawConfig = toml::from_str(text).map_err(|source| ConfigError::Parse {
            path: "test".into(),
            source: Box::new(source),
        })?;
        Config::try_from(raw)
    }

    #[test]
    fn empty_config_is_all_defaults() {
        let config = parse("").expect("empty is valid");
        assert_eq!(config.skin, "koala");
        assert_eq!(config.pets.0.get(), 1);
        assert_eq!(config.scale.0, 1.5);
        assert!(config.click_to_summon);
    }

    #[test]
    fn partial_config_keeps_other_defaults() {
        let config = parse("[app]\npets = 3\n").expect("valid");
        assert_eq!(config.pets.0.get(), 3);
        assert_eq!(config.scale.0, 1.5, "untouched fields keep their default");
    }

    #[test]
    fn full_config_parses() {
        let config = parse(
            r#"
            [app]
            skin = "panda"
            pets = 4
            scale = 2.0
            seed = 99

            [behavior]
            click_to_summon = false
            double_click_ms = 300
            drag_threshold_ms = 100
            "#,
        )
        .expect("valid");
        assert_eq!(config.skin, "panda");
        assert_eq!(config.pets.0.get(), 4);
        assert_eq!(config.scale.0, 2.0);
        assert_eq!(config.seed, Seed(99));
        assert!(!config.click_to_summon);
        assert_eq!(config.gestures.double_click, Duration::from_millis(300));
    }

    /// A typo must be an error, not a silently ignored default.
    #[test]
    fn unknown_keys_are_rejected() {
        assert!(matches!(
            parse("[app]\nwalk_spede = 3\n"),
            Err(ConfigError::Parse { .. })
        ));
        assert!(matches!(
            parse("[nonsense]\nx = 1\n"),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn zero_pets_is_rejected() {
        assert!(matches!(
            parse("[app]\npets = 0\n"),
            Err(ConfigError::PetCount { got: 0 })
        ));
    }

    #[test]
    fn absurd_pet_count_is_rejected() {
        assert!(matches!(
            parse("[app]\npets = 5000\n"),
            Err(ConfigError::PetCount { .. })
        ));
    }

    #[test]
    fn non_positive_scale_is_rejected() {
        assert!(matches!(
            parse("[app]\nscale = 0.0\n"),
            Err(ConfigError::Scale { .. })
        ));
        assert!(matches!(
            parse("[app]\nscale = -2.0\n"),
            Err(ConfigError::Scale { .. })
        ));
    }

    #[test]
    fn zero_durations_are_rejected() {
        assert!(matches!(
            parse("[behavior]\ndouble_click_ms = 0\n"),
            Err(ConfigError::NotPositive {
                field: "double_click_ms"
            })
        ));
    }

    /// A skin name is a directory entry, never a path, so a config cannot point
    /// outside the skins directory.
    #[test]
    fn skin_paths_are_rejected() {
        for bad in ["../../etc", "a/b", "..", ""] {
            let text = format!("[app]\nskin = \"{bad}\"\n");
            assert!(
                matches!(parse(&text), Err(ConfigError::SkinName)),
                "should reject {bad:?}"
            );
        }
    }

    #[test]
    fn missing_file_yields_none_not_an_error() {
        let result = load_config(Path::new("/nonexistent/batates/config.toml"));
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn builtin_name_resolves_to_the_embedded_skin() {
        let config = Config::default();
        let source = config.skin_source(Path::new("/nonexistent/skins"));
        assert_eq!(source, SkinSource::Builtin);
    }

    #[test]
    fn a_user_skin_directory_wins() {
        let skins = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/skins");
        let config = Config {
            skin: "panda".to_string(),
            ..Default::default()
        };
        assert_eq!(
            config.skin_source(&skins),
            SkinSource::Directory(skins.join("panda"))
        );
    }
}
