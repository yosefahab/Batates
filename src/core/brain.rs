//! The pet state machine.
//!
//! The old machine dead-ended: `update_state` matched only `Idle`/`Chilling`
//! and `Sitting`, so `Jumping`, `Eating` and `SendingLove` had no exit. Since
//! `Sitting` led to `Eating` on a timer, every pet ate forever after ~30s.
//!
//! The fix is structural, not a new match arm: exits come only from
//! [`WeightedTable`], which cannot be constructed empty, and nothing in the
//! transition path matches on the state. A dead-end state is unrepresentable.

use bevy::prelude::*;
use serde::Deserialize;
use std::time::Duration;
use thiserror::Error;

use super::rng::PetRng;

/// The eight animation states. Discriminants are the sprite-sheet row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum PetState {
    Chilling = 0,
    Dragged = 1,
    Eating = 2,
    Idle = 3,
    Jumping = 4,
    SendingLove = 5,
    Sitting = 6,
    Walking = 7,
}

impl PetState {
    /// Declaration order is sheet-row order; the sprite sheet is built to match.
    pub const ALL: [PetState; 8] = [
        PetState::Chilling,
        PetState::Dragged,
        PetState::Eating,
        PetState::Idle,
        PetState::Jumping,
        PetState::SendingLove,
        PetState::Sitting,
        PetState::Walking,
    ];

    /// Row index into the sprite sheet.
    pub fn row(self) -> u32 {
        self as u32
    }
}

/// How a state's animation plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playback {
    /// Repeat until something else ends the state.
    Loop,
    /// Play once; finishing is itself a reason to leave the state.
    Once,
}

/// How a state moves the pet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Locomotion {
    Still,
    Walk {
        speed: f32,
    },
    /// Position is owned by the pointer, not by physics.
    Held,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TableError {
    #[error("a transition table must have at least one entry")]
    Empty,
    #[error("a transition table must have total weight greater than zero")]
    ZeroWeight,
}

/// A non-empty weighted choice. Construction is the only place emptiness is
/// checked, so every value of this type is guaranteed to yield an exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeightedTable<T> {
    entries: Vec<(T, u16)>,
    total: u32,
}

impl<T: Copy> WeightedTable<T> {
    pub fn new(entries: Vec<(T, u16)>) -> Result<Self, TableError> {
        if entries.is_empty() {
            return Err(TableError::Empty);
        }
        let total: u32 = entries.iter().map(|(_, w)| u32::from(*w)).sum();
        if total == 0 {
            return Err(TableError::ZeroWeight);
        }
        Ok(Self { entries, total })
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    /// Picks an entry from a roll in `[0, total())`.
    ///
    /// Taking the roll rather than an RNG keeps this pure and lets tests pin
    /// exact boundaries.
    pub fn pick(&self, roll: u32) -> T {
        let mut acc = 0u32;
        for (value, weight) in &self.entries {
            acc += u32::from(*weight);
            if roll < acc {
                return *value;
            }
        }
        // Only reachable if roll >= total, which the contract forbids; the last
        // entry is the safe answer rather than a panic in a hot path.
        self.entries[self.entries.len() - 1].0
    }
}

/// Everything the machine needs to know about one state.
#[derive(Debug, Clone)]
pub struct StateDef {
    pub frames: u32,
    pub fps: u8,
    pub playback: Playback,
    /// Inclusive range the state's duration is drawn from.
    pub duration: (Duration, Duration),
    pub locomotion: Locomotion,
    pub transitions: WeightedTable<PetState>,
}

/// All eight state definitions, indexed by [`PetState::row`].
#[derive(Resource, Debug, Clone)]
pub struct StateTable {
    defs: Vec<StateDef>,
}

impl StateTable {
    /// `defs` must be in [`PetState::ALL`] order.
    pub fn new(defs: Vec<StateDef>) -> Self {
        assert_eq!(defs.len(), PetState::ALL.len(), "one def per state");
        Self { defs }
    }

    pub fn get(&self, state: PetState) -> &StateDef {
        &self.defs[state.row() as usize]
    }
}

/// Per-pet machine state.
#[derive(Component, Debug, Clone)]
pub struct PetBrain {
    pub state: PetState,
    pub elapsed: Duration,
    pub planned: Duration,
    /// While set, timeouts do not fire: an interaction owns the pet.
    pub locked: bool,
}

impl PetBrain {
    pub fn new(state: PetState, planned: Duration) -> Self {
        Self {
            state,
            elapsed: Duration::ZERO,
            planned,
            locked: false,
        }
    }
}

/// Result of one brain tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrainStep {
    Stay,
    Enter(PetState),
}

/// States that own the pet until something explicitly releases it.
fn locks(state: PetState) -> bool {
    matches!(state, PetState::Dragged)
}

/// Advances one pet's brain by `dt`.
///
/// Three things can end a state, and the state's own nature decides which
/// apply: an interrupt (always), reaching a walk target, finishing a `Once`
/// animation, or the planned duration running out. Arrival matters because a
/// pet that reached where it was sent should stop, not keep playing a walk
/// animation on the spot until an unrelated timer expires.
///
/// A locked state ignores everything but an interrupt.
pub fn step_brain(
    brain: &mut PetBrain,
    def: &StateDef,
    interrupt: Option<PetState>,
    playback_finished: bool,
    locomotion_finished: bool,
    dt: Duration,
    rng: &mut PetRng,
) -> BrainStep {
    if let Some(next) = interrupt {
        return enter(brain, next);
    }

    if brain.locked {
        return BrainStep::Stay;
    }

    brain.elapsed += dt;

    let animation_done = def.playback == Playback::Once && playback_finished;
    let walk_done = matches!(def.locomotion, Locomotion::Walk { .. }) && locomotion_finished;
    let timed_out = brain.elapsed >= brain.planned;

    if !(animation_done || walk_done || timed_out) {
        return BrainStep::Stay;
    }

    let roll = rng.roll(def.transitions.total());
    enter(brain, def.transitions.pick(roll))
}

fn enter(brain: &mut PetBrain, next: PetState) -> BrainStep {
    brain.state = next;
    brain.elapsed = Duration::ZERO;
    brain.locked = locks(next);
    BrainStep::Enter(next)
}

/// How long this state's animation takes to play through once.
pub fn animation_length(def: &StateDef) -> Duration {
    if def.fps == 0 {
        return Duration::ZERO;
    }
    Duration::from_secs_f32(def.frames as f32 / f32::from(def.fps))
}

/// Draws the duration for a freshly entered state.
///
/// A `Once` state is never allowed to expire before its animation has played
/// out: leaving early is what cut a 5.1s reaction off after 2s. The drawn
/// duration survives only as a backstop in case the animation never reports
/// finishing, so a state still cannot become terminal.
pub fn plan_duration(def: &StateDef, rng: &mut PetRng) -> Duration {
    let drawn = rng.range_duration(def.duration.0, def.duration.1);
    match def.playback {
        Playback::Loop => drawn,
        Playback::Once => drawn.max(animation_length(def)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::rng::Seed;

    fn secs(s: f32) -> Duration {
        Duration::from_secs_f32(s)
    }

    fn table(entries: &[(PetState, u16)]) -> WeightedTable<PetState> {
        WeightedTable::new(entries.to_vec()).expect("valid table")
    }

    /// A table mirroring the shipped default, used to prove no state dead-ends.
    fn test_table() -> StateTable {
        use PetState::*;
        let def = |frames, playback, lo, hi, locomotion, transitions| StateDef {
            frames,
            fps: 12,
            playback,
            duration: (secs(lo), secs(hi)),
            locomotion,
            transitions,
        };
        StateTable::new(vec![
            def(
                61,
                Playback::Loop,
                4.0,
                12.0,
                Locomotion::Still,
                table(&[(Idle, 3), (Walking, 2), (Sitting, 1)]),
            ),
            def(
                8,
                Playback::Loop,
                0.0,
                0.0,
                Locomotion::Held,
                table(&[(Sitting, 1)]),
            ),
            def(
                24,
                Playback::Once,
                3.0,
                6.0,
                Locomotion::Still,
                table(&[(Idle, 2), (Chilling, 1)]),
            ),
            def(
                38,
                Playback::Loop,
                3.0,
                10.0,
                Locomotion::Still,
                table(&[(Walking, 3), (Chilling, 2), (Sitting, 1), (Eating, 1)]),
            ),
            def(
                11,
                Playback::Once,
                0.9,
                0.9,
                Locomotion::Still,
                table(&[(Idle, 1)]),
            ),
            def(
                61,
                Playback::Once,
                2.0,
                2.0,
                Locomotion::Still,
                table(&[(Idle, 1)]),
            ),
            def(
                1,
                Playback::Loop,
                5.0,
                20.0,
                Locomotion::Still,
                table(&[(Idle, 2), (Eating, 1), (Chilling, 1)]),
            ),
            def(
                8,
                Playback::Loop,
                2.0,
                8.0,
                Locomotion::Walk { speed: 60.0 },
                table(&[(Idle, 3), (Sitting, 1)]),
            ),
        ])
    }

    #[test]
    fn empty_table_is_rejected() {
        assert_eq!(
            WeightedTable::<PetState>::new(vec![]).unwrap_err(),
            TableError::Empty
        );
    }

    #[test]
    fn zero_weight_table_is_rejected() {
        assert_eq!(
            WeightedTable::new(vec![(PetState::Idle, 0), (PetState::Walking, 0)]).unwrap_err(),
            TableError::ZeroWeight
        );
    }

    #[test]
    fn pick_respects_weight_boundaries() {
        let t = table(&[(PetState::Idle, 3), (PetState::Walking, 1)]);
        assert_eq!(t.total(), 4);
        assert_eq!(t.pick(0), PetState::Idle);
        assert_eq!(t.pick(2), PetState::Idle);
        assert_eq!(t.pick(3), PetState::Walking);
    }

    #[test]
    fn interrupt_beats_timeout() {
        let table = test_table();
        let def = table.get(PetState::Idle);
        let mut brain = PetBrain::new(PetState::Idle, secs(100.0));
        let mut rng = PetRng::from_seed(Seed(1));
        let step = step_brain(
            &mut brain,
            def,
            Some(PetState::Dragged),
            false,
            false,
            secs(0.016),
            &mut rng,
        );
        assert_eq!(step, BrainStep::Enter(PetState::Dragged));
        assert!(brain.locked, "Dragged must lock the pet");
    }

    #[test]
    fn locked_state_ignores_timeout() {
        let table = test_table();
        let def = table.get(PetState::Dragged);
        let mut brain = PetBrain::new(PetState::Dragged, Duration::ZERO);
        brain.locked = true;
        let mut rng = PetRng::from_seed(Seed(1));
        for _ in 0..1000 {
            let step = step_brain(&mut brain, def, None, true, false, secs(0.016), &mut rng);
            assert_eq!(step, BrainStep::Stay);
        }
        assert_eq!(brain.state, PetState::Dragged);
    }

    #[test]
    fn once_playback_ends_on_animation() {
        let table = test_table();
        let def = table.get(PetState::Jumping);
        let mut brain = PetBrain::new(PetState::Jumping, secs(999.0));
        let mut rng = PetRng::from_seed(Seed(1));
        // Not finished: stays despite a tiny elapsed time.
        assert_eq!(
            step_brain(&mut brain, def, None, false, false, secs(0.016), &mut rng),
            BrainStep::Stay
        );
        // Finished: leaves even though `planned` is far away.
        assert_eq!(
            step_brain(&mut brain, def, None, true, false, secs(0.016), &mut rng),
            BrainStep::Enter(PetState::Idle)
        );
    }

    /// The direct regression test for the shipped dead-end bug: the old code
    /// could reach Eating and never leave.
    #[test]
    fn no_state_is_terminal() {
        let table = test_table();
        for start in PetState::ALL {
            let mut rng = PetRng::from_seed(Seed(99));
            let mut brain = PetBrain::new(start, secs(0.0));
            let mut seen = std::collections::HashSet::new();
            seen.insert(start);

            for _ in 0..10_000 {
                let def = table.get(brain.state);
                // Release any lock so Dragged is not a false positive; a real
                // drag is ended by an interrupt, which this loop does not model.
                brain.locked = false;
                if let BrainStep::Enter(next) =
                    step_brain(&mut brain, def, None, true, false, secs(0.016), &mut rng)
                {
                    seen.insert(next);
                    brain.planned = plan_duration(table.get(next), &mut rng);
                }
            }

            assert!(
                seen.len() >= 3,
                "starting from {start:?} only ever reached {seen:?}"
            );
        }
    }

    /// Regression test for a Once animation being cut off by its timer: a
    /// 61-frame reaction at 12fps needs ~5.1s, but the manifest asks for 2s.
    #[test]
    fn once_states_are_never_planned_shorter_than_their_animation() {
        let table = test_table();
        let mut rng = PetRng::from_seed(Seed(3));
        for state in [PetState::SendingLove, PetState::Jumping, PetState::Eating] {
            let def = table.get(state);
            let planned = plan_duration(def, &mut rng);
            assert!(
                planned >= animation_length(def),
                "{state:?}: planned {planned:?} < animation {:?}",
                animation_length(def)
            );
        }
    }

    #[test]
    fn a_once_state_plays_to_the_end_before_leaving() {
        let table = test_table();
        let def = table.get(PetState::SendingLove);
        let mut rng = PetRng::from_seed(Seed(4));
        let mut brain = PetBrain::new(PetState::SendingLove, plan_duration(def, &mut rng));

        // Tick well past the drawn 2s duration without the animation finishing.
        let mut elapsed = Duration::ZERO;
        while elapsed < secs(4.0) {
            let step = step_brain(&mut brain, def, None, false, false, secs(0.05), &mut rng);
            assert_eq!(step, BrainStep::Stay, "left early at {elapsed:?}");
            elapsed += secs(0.05);
        }

        // It leaves as soon as the animation reports finishing.
        assert_eq!(
            step_brain(&mut brain, def, None, true, false, secs(0.05), &mut rng),
            BrainStep::Enter(PetState::Idle)
        );
    }

    /// Regression test for a pet that reached its destination but kept playing
    /// the walk animation in place until an unrelated timer expired.
    #[test]
    fn arriving_ends_a_walk_immediately() {
        let table = test_table();
        let def = table.get(PetState::Walking);
        let mut rng = PetRng::from_seed(Seed(8));
        let mut brain = PetBrain::new(PetState::Walking, secs(999.0));

        // Still travelling: the long duration keeps it walking.
        assert_eq!(
            step_brain(&mut brain, def, None, false, false, secs(0.05), &mut rng),
            BrainStep::Stay
        );
        // Arrived: it leaves at once rather than waiting out the clock.
        assert!(matches!(
            step_brain(&mut brain, def, None, false, true, secs(0.05), &mut rng),
            BrainStep::Enter(_)
        ));
    }

    /// Arrival is meaningless for a state that does not move, and must not end
    /// it early.
    #[test]
    fn arrival_does_not_end_a_still_state() {
        let table = test_table();
        let def = table.get(PetState::Idle);
        let mut rng = PetRng::from_seed(Seed(8));
        let mut brain = PetBrain::new(PetState::Idle, secs(999.0));
        assert_eq!(
            step_brain(&mut brain, def, None, false, true, secs(0.05), &mut rng),
            BrainStep::Stay
        );
    }

    #[test]
    fn seeded_runs_are_reproducible() {
        let table = test_table();
        let run = || {
            let mut rng = PetRng::from_seed(Seed(2024));
            let mut brain = PetBrain::new(PetState::Idle, secs(0.0));
            let mut trace = Vec::new();
            for _ in 0..200 {
                let def = table.get(brain.state);
                brain.locked = false;
                if let BrainStep::Enter(next) =
                    step_brain(&mut brain, def, None, true, false, secs(0.1), &mut rng)
                {
                    trace.push(next);
                    brain.planned = plan_duration(table.get(next), &mut rng);
                }
            }
            trace
        };
        assert_eq!(run(), run());
    }
}
