//! Skin manifests: the on-disk description of a sprite sheet and its behaviour.
//!
//! The sheet is a strict `rows x columns` grid. A state's row is its position in
//! the `states` list, which is why the list must be complete and in
//! [`PetState::ALL`] order. That is what turns the old hand-maintained table of
//! absolute frame indices into arithmetic.
//!
//! Serde types live here rather than in `core` so the gameplay logic stays free
//! of serialisation concerns; [`SkinManifest::into_parts`] is the boundary where
//! untrusted file contents become validated domain types.

use bevy::prelude::*;
use serde::Deserialize;
use std::time::Duration;
use thiserror::Error;

use crate::core::brain::{
    Locomotion, PetState, Playback, StateDef, StateTable, TableError, WeightedTable,
};

#[derive(Debug, Error)]
pub enum SkinError {
    #[error("could not read skin manifest at {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse skin manifest at {path}: {source}")]
    Parse {
        path: String,
        // Boxed: SpannedError is large enough to bloat every Result in this
        // module if stored inline.
        #[source]
        source: Box<ron::error::SpannedError>,
    },
    #[error("skin declares {got} states but exactly {want} are required")]
    StateCount { got: usize, want: usize },
    #[error("skin state at index {index} is {got:?} but must be {want:?}")]
    StateOrder {
        index: usize,
        got: PetState,
        want: PetState,
    },
    #[error(
        "state {state:?} declares {frames} frames, which exceeds the sheet's {columns} columns"
    )]
    TooManyFrames {
        state: PetState,
        frames: u32,
        columns: u32,
    },
    #[error("state {state:?} must declare at least one frame")]
    NoFrames { state: PetState },
    #[error("state {state:?} has duration min {min}s greater than max {max}s")]
    BadDuration { state: PetState, min: f32, max: f32 },
    #[error("state {state:?} has an unusable transition table: {source}")]
    BadTransitions {
        state: PetState,
        #[source]
        source: TableError,
    },
    #[error(
        "skin declares columns={columns} and frame width {width}, but the sheet is {actual}px wide"
    )]
    SheetWidth {
        columns: u32,
        width: u32,
        actual: u32,
    },
    #[error(
        "skin declares {rows} rows and frame height {height}, but the sheet is {actual}px tall"
    )]
    SheetHeight { rows: u32, height: u32, actual: u32 },
}

/// How a state moves, as written in the manifest.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub enum LocomotionSpec {
    Still,
    Held,
    Walk { speed: f32 },
}

impl From<LocomotionSpec> for Locomotion {
    fn from(spec: LocomotionSpec) -> Self {
        match spec {
            LocomotionSpec::Still => Locomotion::Still,
            LocomotionSpec::Held => Locomotion::Held,
            LocomotionSpec::Walk { speed } => Locomotion::Walk { speed },
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub enum PlaybackSpec {
    Loop,
    Once,
}

impl From<PlaybackSpec> for Playback {
    fn from(spec: PlaybackSpec) -> Self {
        match spec {
            PlaybackSpec::Loop => Playback::Loop,
            PlaybackSpec::Once => Playback::Once,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct TransitionSpec {
    pub to: PetState,
    pub weight: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StateSpec {
    pub state: PetState,
    pub frames: u32,
    /// Overrides the skin-wide default when a state needs its own pace.
    #[serde(default)]
    pub fps: Option<u8>,
    pub playback: PlaybackSpec,
    /// Seconds, inclusive range the state's duration is drawn from.
    pub duration: (f32, f32),
    pub locomotion: LocomotionSpec,
    pub transitions: Vec<TransitionSpec>,
}

/// A skin as written on disk.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkinManifest {
    pub name: String,
    /// Sheet filename, resolved relative to the manifest's own directory.
    pub sheet: String,
    pub frame_size: (u32, u32),
    pub columns: u32,
    pub default_fps: u8,
    pub states: Vec<StateSpec>,
}

/// The validated visual half of a skin.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinGeometry {
    pub name: String,
    pub sheet: String,
    pub frame_size: UVec2,
    pub columns: u32,
    pub rows: u32,
}

impl SkinManifest {
    pub fn parse(text: &str, path: &str) -> Result<Self, SkinError> {
        ron::from_str(text).map_err(|source| SkinError::Parse {
            path: path.to_string(),
            source: Box::new(source),
        })
    }

    /// Validates the manifest and splits it into geometry and a behaviour table.
    ///
    /// Every failure mode is checked here, at the boundary, so nothing
    /// downstream has to defend against a malformed skin.
    pub fn into_parts(self) -> Result<(SkinGeometry, StateTable), SkinError> {
        if self.states.len() != PetState::ALL.len() {
            return Err(SkinError::StateCount {
                got: self.states.len(),
                want: PetState::ALL.len(),
            });
        }

        let mut defs = Vec::with_capacity(self.states.len());
        for (index, spec) in self.states.iter().enumerate() {
            let want = PetState::ALL[index];
            if spec.state != want {
                return Err(SkinError::StateOrder {
                    index,
                    got: spec.state,
                    want,
                });
            }
            if spec.frames == 0 {
                return Err(SkinError::NoFrames { state: spec.state });
            }
            if spec.frames > self.columns {
                return Err(SkinError::TooManyFrames {
                    state: spec.state,
                    frames: spec.frames,
                    columns: self.columns,
                });
            }
            let (min, max) = spec.duration;
            if min > max {
                return Err(SkinError::BadDuration {
                    state: spec.state,
                    min,
                    max,
                });
            }

            let entries = spec
                .transitions
                .iter()
                .map(|t| (t.to, t.weight))
                .collect::<Vec<_>>();
            let transitions =
                WeightedTable::new(entries).map_err(|source| SkinError::BadTransitions {
                    state: spec.state,
                    source,
                })?;

            defs.push(StateDef {
                frames: spec.frames,
                fps: spec.fps.unwrap_or(self.default_fps),
                playback: spec.playback.into(),
                duration: (
                    Duration::from_secs_f32(min.max(0.0)),
                    Duration::from_secs_f32(max.max(0.0)),
                ),
                locomotion: spec.locomotion.into(),
                transitions,
            });
        }

        let geometry = SkinGeometry {
            name: self.name,
            sheet: self.sheet,
            frame_size: UVec2::new(self.frame_size.0, self.frame_size.1),
            columns: self.columns,
            rows: PetState::ALL.len() as u32,
        };

        Ok((geometry, StateTable::new(defs)))
    }
}

impl SkinGeometry {
    /// Checks the loaded sheet actually matches what the manifest promised.
    ///
    /// Without this a wrong `columns` value produces silently misaligned
    /// animation rather than an error.
    pub fn verify_sheet(&self, actual: UVec2) -> Result<(), SkinError> {
        let want_width = self.columns * self.frame_size.x;
        if actual.x != want_width {
            return Err(SkinError::SheetWidth {
                columns: self.columns,
                width: self.frame_size.x,
                actual: actual.x,
            });
        }
        let want_height = self.rows * self.frame_size.y;
        if actual.y != want_height {
            return Err(SkinError::SheetHeight {
                rows: self.rows,
                height: self.frame_size.y,
                actual: actual.y,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The manifest shipped with the built-in koala, kept in sync by a test
    /// below that parses the real file.
    fn valid_ron() -> String {
        ron_with_states(&PetState::ALL)
    }

    /// Builds a manifest whose state list is exactly `order`, so tests can
    /// construct malformed input without string surgery.
    fn ron_with_states(order: &[PetState]) -> String {
        let mut states = String::new();
        for &state in order {
            states.push_str(&format!(
                "(state: {state:?}, frames: 8, playback: Loop, duration: (1.0, 2.0), \
                 locomotion: Still, transitions: [(to: Idle, weight: 1)]),\n"
            ));
        }
        format!(
            "SkinManifest(name: \"t\", sheet: \"s.png\", frame_size: (50, 50), \
             columns: 61, default_fps: 12, states: [{states}])"
        )
    }

    fn parse(text: &str) -> Result<(SkinGeometry, StateTable), SkinError> {
        SkinManifest::parse(text, "test")?.into_parts()
    }

    #[test]
    fn valid_manifest_parses() {
        let (geometry, table) = parse(&valid_ron()).expect("valid");
        assert_eq!(geometry.columns, 61);
        assert_eq!(geometry.rows, 8);
        assert_eq!(geometry.frame_size, UVec2::splat(50));
        assert_eq!(table.get(PetState::Walking).frames, 8);
    }

    #[test]
    fn per_state_fps_overrides_the_default() {
        let text = valid_ron().replace(
            "(state: Sitting, frames: 8, playback: Loop",
            "(state: Sitting, frames: 8, fps: Some(1), playback: Loop",
        );
        let (_, table) = parse(&text).expect("valid");
        assert_eq!(table.get(PetState::Sitting).fps, 1);
        assert_eq!(
            table.get(PetState::Walking).fps,
            12,
            "default still applies"
        );
    }

    #[test]
    fn missing_state_is_rejected() {
        let text = valid_ron().replace(
            "(state: Sitting, frames: 8, playback: Loop, duration: (1.0, 2.0), locomotion: Still, transitions: [(to: Idle, weight: 1)]),\n",
            "",
        );
        assert!(matches!(
            parse(&text),
            Err(SkinError::StateCount { got: 7, want: 8 })
        ));
    }

    /// Row order is load-bearing: the sheet row is the state's index, so a
    /// reordered list would animate every state with the wrong frames.
    #[test]
    fn out_of_order_states_are_rejected() {
        let mut order = PetState::ALL;
        order.swap(0, 7);
        let text = ron_with_states(&order);

        match parse(&text) {
            Err(SkinError::StateOrder { index, got, want }) => {
                assert_eq!(index, 0);
                assert_eq!(got, PetState::Walking);
                assert_eq!(want, PetState::Chilling);
            }
            other => panic!("expected a StateOrder error, got {other:?}"),
        }
    }

    #[test]
    fn frames_beyond_the_column_count_are_rejected() {
        let text = valid_ron().replace("frames: 8", "frames: 99");
        assert!(matches!(parse(&text), Err(SkinError::TooManyFrames { .. })));
    }

    #[test]
    fn zero_frames_is_rejected() {
        let text = valid_ron().replacen("frames: 8", "frames: 0", 1);
        assert!(matches!(parse(&text), Err(SkinError::NoFrames { .. })));
    }

    #[test]
    fn inverted_duration_is_rejected() {
        let text = valid_ron().replacen("duration: (1.0, 2.0)", "duration: (5.0, 2.0)", 1);
        assert!(matches!(parse(&text), Err(SkinError::BadDuration { .. })));
    }

    /// The structural guarantee against dead-end states, enforced at the file
    /// boundary rather than trusted.
    #[test]
    fn empty_transitions_are_rejected() {
        let text =
            valid_ron().replacen("transitions: [(to: Idle, weight: 1)]", "transitions: []", 1);
        assert!(matches!(
            parse(&text),
            Err(SkinError::BadTransitions {
                source: TableError::Empty,
                ..
            })
        ));
    }

    #[test]
    fn zero_weight_transitions_are_rejected() {
        let text = valid_ron().replacen(
            "transitions: [(to: Idle, weight: 1)]",
            "transitions: [(to: Idle, weight: 0)]",
            1,
        );
        assert!(matches!(
            parse(&text),
            Err(SkinError::BadTransitions {
                source: TableError::ZeroWeight,
                ..
            })
        ));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let text = valid_ron().replace("name: \"t\"", "name: \"t\", nonsense: 1");
        assert!(matches!(parse(&text), Err(SkinError::Parse { .. })));
    }

    #[test]
    fn sheet_dimensions_are_verified() {
        let (geometry, _) = parse(&valid_ron()).expect("valid");
        // 61 columns x 50px = 3050, 8 rows x 50px = 400: the real koala sheet.
        assert!(geometry.verify_sheet(UVec2::new(3050, 400)).is_ok());
        assert!(matches!(
            geometry.verify_sheet(UVec2::new(2450, 400)),
            Err(SkinError::SheetWidth { .. })
        ));
        assert!(matches!(
            geometry.verify_sheet(UVec2::new(3050, 350)),
            Err(SkinError::SheetHeight { .. })
        ));
    }
}
