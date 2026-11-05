//! Pure game rules that every client derives the same way: roles, star counts, twists and
//! starting positions.

use crate::fair;
use crate::wire::{LOOM_H, LOOM_W, Pos};

/// Most rounds in a night.
pub const MAX_ROUNDS: usize = 8;
/// A figure has at least this many points, so lone Threaders hold several stars.
pub const MIN_POINTS: usize = 4;
/// Last part of the weave with the fastest star sync.
pub const FIRST_LIGHT_MS: u64 = 10_000;
/// Twin Weavers needs this many players.
pub const TWIN_MIN_PLAYERS: usize = 8;
/// Fog: a Threader sees other stars only within this distance of their own.
pub const FOG_RADIUS: u32 = 320;

/// The optional twist of a round ("Skies").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Twist {
    Calm,
    /// Threaders only see stars near their own.
    Fog,
    /// The Weaver sees the figure mirrored and must describe it flipped.
    Mirror,
    /// One star slowly drifts and must be held in place.
    Drift,
    /// Two Weavers each see half of the figure.
    Twin,
}

impl Twist {
    pub fn key(self) -> &'static str {
        match self {
            Twist::Calm => "calm",
            Twist::Fog => "fog",
            Twist::Mirror => "mirror",
            Twist::Drift => "drift",
            Twist::Twin => "twin",
        }
    }
}

/// The twist of round `round` (1-based). Public and known in advance: twists change how the
/// round plays, not who wins.
pub fn twist(night: u32, round: u8, mixed: bool, players: usize) -> Twist {
    if !mixed || round < 3 {
        return Twist::Calm;
    }
    let mut options = vec![Twist::Fog, Twist::Mirror, Twist::Drift];
    if players >= TWIN_MIN_PLAYERS {
        options.push(Twist::Twin);
    }
    options[fair::public_draw(night, round, b"twist") as usize % options.len()]
}

/// The second Weaver in a Twin Weavers round: the next player after the Weaver.
pub fn twin_of(players: &[u32], weaver: u32) -> Option<u32> {
    let i = players.iter().position(|&p| p == weaver)?;
    let twin = players[(i + 1) % players.len()];
    (twin != weaver).then_some(twin)
}

/// Stars each Threader holds, so the figure has at least [`MIN_POINTS`] points.
pub fn stars_each(threaders: usize) -> usize {
    if threaders == 0 {
        0
    } else {
        MIN_POINTS.div_ceil(threaders).max(1)
    }
}

/// Starting positions of all stars, spread across the middle of the loom in Threader order.
pub fn start_positions(total: usize) -> Vec<Pos> {
    let margin = 220u32;
    let span = u32::from(LOOM_W) - 2 * margin;
    (0..total)
        .map(|k| {
            let x = if total <= 1 {
                u32::from(LOOM_W) / 2
            } else {
                margin + span * k as u32 / (total as u32 - 1)
            };
            let y = u32::from(LOOM_H) / 2 + if k % 2 == 0 { 70 } else { 0 } - 35;
            (x as u16, y as u16)
        })
        .collect()
}

/// Drift: which star (index among all stars of the round) drifts, and its step per second.
pub fn drift(night: u32, round: u8, total: usize) -> Option<(usize, (i32, i32))> {
    if total == 0 {
        return None;
    }
    let draw = fair::public_draw(night, round, b"drift");
    // Eight directions, 14 loom units per second.
    const STEPS: [(i32, i32); 8] = [
        (14, 0),
        (10, 10),
        (0, 14),
        (-10, 10),
        (-14, 0),
        (-10, -10),
        (0, -14),
        (10, -10),
    ];
    Some((draw as usize % total, STEPS[(draw >> 8) as usize % 8]))
}

/// Clamps a position onto the loom.
pub fn clamp(x: i32, y: i32) -> Pos {
    (
        x.clamp(0, i32::from(LOOM_W)) as u16,
        y.clamp(0, i32::from(LOOM_H)) as u16,
    )
}

/// Time bonus for tying off early: one point per 4 spare seconds, at most 5.
pub fn tie_off_bonus(spare: u8) -> u8 {
    (spare / 4).min(5)
}

/// Mirrors a figure point for the Mirror twist (left and right swap).
pub fn mirror((x, y): (i32, i32)) -> (i32, i32) {
    (i32::from(LOOM_W) - x, y)
}
