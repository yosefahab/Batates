//! Randomness as an injected dependency.
//!
//! Gameplay must never call `rand::random` or read ambient entropy directly:
//! a seeded run has to reproduce exactly, or the state machine cannot be tested.

use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::time::Duration;

/// A run seed. Zero means "draw from OS entropy"; any other value is reproducible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seed(pub u64);

/// The single source of randomness for gameplay.
///
/// ChaCha8 rather than the thread RNG because it is reproducible across
/// platforms and versions, which `SmallRng` explicitly does not promise.
#[derive(Resource, Debug)]
pub struct PetRng(ChaCha8Rng);

impl PetRng {
    pub fn from_seed(seed: Seed) -> Self {
        match seed.0 {
            0 => Self(ChaCha8Rng::from_os_rng()),
            n => Self(ChaCha8Rng::seed_from_u64(n)),
        }
    }

    /// A roll in `[0, bound)`. `bound` must be non-zero.
    pub fn roll(&mut self, bound: u32) -> u32 {
        self.0.random_range(0..bound)
    }

    /// A duration uniformly between `lo` and `hi` inclusive.
    /// Returns `lo` when `hi <= lo`, so a fixed-duration state needs no special case.
    pub fn range_duration(&mut self, lo: Duration, hi: Duration) -> Duration {
        if hi <= lo {
            return lo;
        }
        let span = (hi - lo).as_secs_f64();
        lo + Duration::from_secs_f64(self.0.random_range(0.0..=span))
    }

    /// A point uniformly inside `half_extent` of the origin, for wander targets.
    pub fn point_in(&mut self, half_extent: Vec2) -> Vec2 {
        Vec2::new(
            self.0.random_range(-half_extent.x..=half_extent.x),
            self.0.random_range(-half_extent.y..=half_extent.y),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_gives_same_sequence() {
        let mut a = PetRng::from_seed(Seed(42));
        let mut b = PetRng::from_seed(Seed(42));
        let seq_a: Vec<u32> = (0..32).map(|_| a.roll(1000)).collect();
        let seq_b: Vec<u32> = (0..32).map(|_| b.roll(1000)).collect();
        assert_eq!(seq_a, seq_b);
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = PetRng::from_seed(Seed(1));
        let mut b = PetRng::from_seed(Seed(2));
        let seq_a: Vec<u32> = (0..32).map(|_| a.roll(1000)).collect();
        let seq_b: Vec<u32> = (0..32).map(|_| b.roll(1000)).collect();
        assert_ne!(seq_a, seq_b);
    }

    #[test]
    fn roll_stays_in_bounds() {
        let mut rng = PetRng::from_seed(Seed(7));
        for _ in 0..1000 {
            assert!(rng.roll(5) < 5);
        }
    }

    #[test]
    fn degenerate_duration_range_returns_lo() {
        let mut rng = PetRng::from_seed(Seed(7));
        let d = rng.range_duration(Duration::from_secs(2), Duration::from_secs(2));
        assert_eq!(d, Duration::from_secs(2));
    }

    #[test]
    fn duration_stays_in_range() {
        let mut rng = PetRng::from_seed(Seed(7));
        let (lo, hi) = (Duration::from_secs(1), Duration::from_secs(4));
        for _ in 0..500 {
            let d = rng.range_duration(lo, hi);
            assert!(d >= lo && d <= hi, "{d:?}");
        }
    }
}
