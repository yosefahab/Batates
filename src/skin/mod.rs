//! Loading skins from disk, with the koala embedded as a fallback.
//!
//! Skins are read once at startup with `std::fs` rather than through
//! `AssetServer`. A pet does not need hot-reload, and going through the asset
//! pipeline would mean either a custom `AssetSource` or relaxing Bevy 0.19's
//! `unapproved_path_mode`, which defaults to `Forbid` precisely to stop a file
//! referencing paths outside its own root. Reading and validating at one
//! boundary is both smaller and easier to reason about.

pub mod manifest;

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use std::path::{Path, PathBuf};

use crate::core::brain::StateTable;
use manifest::{SkinError, SkinGeometry, SkinManifest};

/// The built-in skin, compiled in so the app runs with nothing installed.
const BUILTIN_MANIFEST: &str = include_str!("../../assets/builtin/koala/skin.ron");
const BUILTIN_SHEET: &[u8] = include_bytes!("../../assets/builtin/koala/sheet.png");

/// Which skin to load.
// Directory is selected by the config file; only tests reach it today.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkinSource {
    /// The embedded koala.
    Builtin,
    /// A directory holding `skin.ron` and its sheet.
    Directory(PathBuf),
}

/// A loaded skin's visual half. The behaviour half becomes [`StateTable`].
#[derive(Resource, Debug, Clone)]
pub struct Skin {
    pub geometry: SkinGeometry,
    pub image: Handle<Image>,
    pub layout: Handle<TextureAtlasLayout>,
}

impl Skin {
    /// Frame size in pixels, as a float vector for hitbox maths.
    pub fn frame_size(&self) -> Vec2 {
        self.geometry.frame_size.as_vec2()
    }

    pub fn columns(&self) -> u32 {
        self.geometry.columns
    }
}

/// A skin's raw contents, before any Bevy assets exist.
struct RawSkin {
    geometry: SkinGeometry,
    table: StateTable,
    sheet_bytes: Vec<u8>,
}

/// Reads and validates a skin. Every failure is reported, never defaulted away.
fn read_skin(source: &SkinSource) -> Result<RawSkin, SkinError> {
    let (text, path, sheet_bytes) = match source {
        SkinSource::Builtin => (
            BUILTIN_MANIFEST.to_string(),
            "<builtin>".to_string(),
            BUILTIN_SHEET.to_vec(),
        ),
        SkinSource::Directory(dir) => {
            let manifest_path = dir.join("skin.ron");
            let text = read_file(&manifest_path)?;
            // The sheet name is read from the manifest below, so this arm needs
            // a second pass; parse first, then load the sheet it names.
            let manifest = SkinManifest::parse(&text, &manifest_path.display().to_string())?;
            let sheet_path = dir.join(&manifest.sheet);
            let bytes = read_bytes(&sheet_path)?;
            (text, manifest_path.display().to_string(), bytes)
        }
    };

    let manifest = SkinManifest::parse(&text, &path)?;
    let (geometry, table) = manifest.into_parts()?;

    Ok(RawSkin {
        geometry,
        table,
        sheet_bytes,
    })
}

fn read_file(path: &Path) -> Result<String, SkinError> {
    std::fs::read_to_string(path).map_err(|source| SkinError::Read {
        path: path.display().to_string(),
        source,
    })
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, SkinError> {
    std::fs::read(path).map_err(|source| SkinError::Read {
        path: path.display().to_string(),
        source,
    })
}

/// Turns validated skin data into Bevy assets.
///
/// Returns an error rather than panicking so the caller can fall back to the
/// built-in skin when a user skin is broken.
fn build_skin(
    raw: RawSkin,
    images: &mut Assets<Image>,
    layouts: &mut Assets<TextureAtlasLayout>,
) -> Result<(Skin, StateTable), SkinError> {
    let image = Image::from_buffer(
        &raw.sheet_bytes,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::nearest(),
        RenderAssetUsages::default(),
    )
    .map_err(|e| SkinError::Read {
        path: raw.geometry.sheet.clone(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
    })?;

    raw.geometry.verify_sheet(image.size())?;

    let layout = layouts.add(TextureAtlasLayout::from_grid(
        raw.geometry.frame_size,
        raw.geometry.columns,
        raw.geometry.rows,
        None,
        None,
    ));

    Ok((
        Skin {
            geometry: raw.geometry,
            image: images.add(image),
            layout,
        },
        raw.table,
    ))
}

/// Loads `source`, falling back to the built-in skin if it fails.
///
/// A broken user skin is a recoverable condition: the app is still useful with
/// the default pet, and refusing to start would be a worse experience than a
/// loud warning. A broken *built-in* skin is a build error, so that panics.
pub fn load_or_builtin(
    source: &SkinSource,
    images: &mut Assets<Image>,
    layouts: &mut Assets<TextureAtlasLayout>,
) -> (Skin, StateTable) {
    match read_skin(source).and_then(|raw| build_skin(raw, images, layouts)) {
        Ok(loaded) => loaded,
        Err(error) => {
            if *source == SkinSource::Builtin {
                panic!("the built-in skin must be valid: {error}");
            }
            warn!("{error}; falling back to the built-in skin");
            let raw = read_skin(&SkinSource::Builtin).expect("built-in skin is valid");
            build_skin(raw, images, layouts).expect("built-in skin is valid")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::brain::PetState;

    /// The shipped manifest must parse and match the shipped sheet. This is the
    /// test that keeps `assets/builtin/koala/` honest.
    #[test]
    fn builtin_skin_is_valid() {
        let raw = read_skin(&SkinSource::Builtin).expect("built-in skin parses");
        assert_eq!(raw.geometry.columns, 61);
        assert_eq!(raw.geometry.rows, 8);
        assert_eq!(raw.table.get(PetState::Chilling).frames, 61);
        assert_eq!(raw.table.get(PetState::Sitting).frames, 1);
        assert_eq!(raw.table.get(PetState::Walking).frames, 8);
        // The koala sheet is 3050x400.
        raw.geometry
            .verify_sheet(UVec2::new(3050, 400))
            .expect("sheet matches the manifest");
    }

    /// The whole point of the manifest: a skin with a different column count
    /// loads without a code change. Panda is 49 columns to koala's 61.
    #[test]
    fn panda_skin_has_its_own_column_count() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/skins/panda");
        let raw = read_skin(&SkinSource::Directory(dir)).expect("panda skin parses");
        assert_eq!(raw.geometry.columns, 49);
        assert_eq!(raw.table.get(PetState::Eating).frames, 32);
        assert_eq!(raw.table.get(PetState::Sitting).frames, 4);
        raw.geometry
            .verify_sheet(UVec2::new(2450, 400))
            .expect("sheet matches the manifest");
    }

    /// The shipped skins, not just a test fixture, must be free of dead ends.
    /// This is the file-level guard against the bug where a pet reached Eating
    /// and stayed there forever.
    #[test]
    fn shipped_skins_have_no_dead_end_states() {
        use crate::core::brain::{BrainStep, PetBrain, plan_duration, step_brain};
        use crate::core::rng::{PetRng, Seed};
        use std::time::Duration;

        let panda = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/skins/panda");
        for source in [SkinSource::Builtin, SkinSource::Directory(panda)] {
            let table = read_skin(&source).expect("skin parses").table;
            for start in PetState::ALL {
                let mut rng = PetRng::from_seed(Seed(11));
                let mut brain = PetBrain::new(start, Duration::ZERO);
                let mut seen = std::collections::HashSet::new();
                for _ in 0..5_000 {
                    // Dragged is released by an interrupt, not a timeout, so
                    // clear the lock to avoid a false positive.
                    brain.locked = false;
                    let def = table.get(brain.state);
                    if let BrainStep::Enter(next) = step_brain(
                        &mut brain,
                        def,
                        None,
                        true,
                        false,
                        Duration::from_millis(50),
                        &mut rng,
                    ) {
                        seen.insert(next);
                        brain.planned = plan_duration(table.get(next), &mut rng);
                    }
                }
                assert!(
                    seen.len() >= 3,
                    "{source:?} starting at {start:?} reached only {seen:?}"
                );
            }
        }
    }

    #[test]
    fn missing_skin_directory_is_an_error_not_a_panic() {
        let dir = PathBuf::from("/nonexistent/skin/dir");
        assert!(matches!(
            read_skin(&SkinSource::Directory(dir)),
            Err(SkinError::Read { .. })
        ));
    }
}
