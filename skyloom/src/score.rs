//! Resonance: how close the stars sit to the figure's points, after removing position and size.
//!
//! Every client must get bit-identical results from the same inputs, so everything here is
//! integer maths. Rotation is not removed on purpose: "up" means up, which makes descriptions
//! like "the tail goes down-right" meaningful.

/// A point on the loom, in loom units (see [`crate::wire::LOOM_W`]).
pub type Point = (i32, i32);

/// Fixed-point scale of the normalised shapes: a distance of `UNIT` equals the shape's RMS radius.
const UNIT: i64 = 1 << 16;
/// Normalised distance at which a star scores 0 %: 0.9 RMS radii.
const ZERO_AT: i64 = UNIT * 9 / 10;
/// Round resonance from which a constellation is woven into the sky.
pub const WOVEN: u8 = 60;
/// Round resonance from which it is woven with gold thread.
pub const GOLD: u8 = 90;

/// The scored round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Score {
    /// For each star, the index of the figure point it was matched to.
    pub matched: Vec<usize>,
    /// Each star's resonance, 0–100.
    pub stars: Vec<u8>,
    /// The round's resonance, 0–100: the mean of the stars'.
    pub resonance: u8,
}

/// Integer square root (floor).
pub fn isqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// Moves the shape's centroid to the origin and scales its RMS radius to [`UNIT`]. None when all
/// points coincide (there is no shape to compare).
fn normalise(points: &[Point]) -> Option<Vec<(i64, i64)>> {
    let n = points.len() as i64;
    let (sx, sy) = points.iter().fold((0i64, 0i64), |(x, y), p| {
        (x + i64::from(p.0), y + i64::from(p.1))
    });
    // Scaled by n so the centroid needs no division.
    let centred: Vec<(i64, i64)> = points
        .iter()
        .map(|p| (n * i64::from(p.0) - sx, n * i64::from(p.1) - sy))
        .collect();
    let sum_sq: u64 = centred.iter().map(|&(x, y)| (x * x + y * y) as u64).sum();
    let rms = isqrt(sum_sq / n as u64) as i64;
    if rms == 0 {
        return None;
    }
    Some(
        centred
            .into_iter()
            .map(|(x, y)| (x * UNIT / rms, y * UNIT / rms))
            .collect(),
    )
}

/// Scores `stars` against the same number of figure `points`. Returns None when the counts
/// differ or there are fewer than two stars.
pub fn score(stars: &[Point], points: &[Point]) -> Option<Score> {
    let n = stars.len();
    if n < 2 || n != points.len() || n > 16 {
        return None;
    }
    let figure = normalise(points)?;
    let Some(sky) = normalise(stars) else {
        // All stars on one spot: nothing woven.
        return Some(Score {
            matched: (0..n).collect(),
            stars: vec![0; n],
            resonance: 0,
        });
    };
    let cost: Vec<Vec<u64>> = sky
        .iter()
        .map(|s| {
            figure
                .iter()
                .map(|f| ((s.0 - f.0).pow(2) + (s.1 - f.1).pow(2)) as u64)
                .collect()
        })
        .collect();
    let matched = assign(&cost);
    let resonances: Vec<u8> = matched
        .iter()
        .enumerate()
        .map(|(i, &j)| star_resonance(isqrt(cost[i][j]) as i64))
        .collect();
    let total: u32 = resonances.iter().map(|&r| u32::from(r)).sum();
    Some(Score {
        matched,
        stars: resonances,
        resonance: (total / n as u32) as u8,
    })
}

fn star_resonance(distance: i64) -> u8 {
    (100 - (distance * 100 / ZERO_AT).min(100)) as u8
}

/// The assignment of rows to columns with the least total cost (exact, by dynamic programming
/// over the set of used columns; fine for up to 16 points). Ties go to the lower column index,
/// so every client picks the same one.
pub fn assign(cost: &[Vec<u64>]) -> Vec<usize> {
    let n = cost.len();
    let full = 1usize << n;
    let mut best = vec![u64::MAX; full];
    let mut choice = vec![0u8; full];
    best[0] = 0;
    for mask in 0..full {
        if best[mask] == u64::MAX {
            continue;
        }
        let row = mask.count_ones() as usize;
        if row == n {
            continue;
        }
        for (col, &c) in cost[row].iter().enumerate() {
            if mask & (1 << col) != 0 {
                continue;
            }
            let next = mask | (1 << col);
            let total = best[mask] + c;
            if total < best[next] {
                best[next] = total;
                choice[next] = col as u8;
            }
        }
    }
    let mut matched = vec![0; n];
    let mut mask = full - 1;
    for row in (0..n).rev() {
        let col = usize::from(choice[mask]);
        matched[row] = col;
        mask &= !(1 << col);
    }
    matched
}

/// Maps figure `points` onto the stars' frame (same centroid and RMS radius as `stars`), for
/// drawing the ghost of the true figure at dawn. Display only, so floats are fine.
pub fn fit(points: &[Point], stars: &[Point]) -> Option<(f64, f64, f64, f64, f64)> {
    fn frame(p: &[Point]) -> Option<(f64, f64, f64)> {
        if p.is_empty() {
            return None;
        }
        let n = p.len() as f64;
        let cx = p.iter().map(|q| f64::from(q.0)).sum::<f64>() / n;
        let cy = p.iter().map(|q| f64::from(q.1)).sum::<f64>() / n;
        let r = (p
            .iter()
            .map(|q| (f64::from(q.0) - cx).powi(2) + (f64::from(q.1) - cy).powi(2))
            .sum::<f64>()
            / n)
            .sqrt();
        Some((cx, cy, r))
    }
    let (fx, fy, fr) = frame(points)?;
    let (sx, sy, sr) = frame(stars)?;
    if fr == 0.0 || sr == 0.0 {
        return None;
    }
    // x' = (x - fx) * k + sx
    Some((fx, fy, sr / fr, sx, sy))
}

/// The figure point a star at `p` would need to be to fit perfectly: used for the hint ring in
/// the reveal. Applies [`fit`]'s transform.
pub fn apply(t: (f64, f64, f64, f64, f64), p: Point) -> (f64, f64) {
    let (fx, fy, k, sx, sy) = t;
    (
        (f64::from(p.0) - fx) * k + sx,
        (f64::from(p.1) - fy) * k + sy,
    )
}
