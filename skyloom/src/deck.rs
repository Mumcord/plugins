//! The figure deck: 60 original constellations, each a line drawing on a 100 × 100 grid
//! (y grows downwards). A figure is sampled into as many points as there are Threader stars by
//! walking its lines at equal distances, so one drawing serves every player count.

use crate::score::{Point, isqrt};

/// One figure: its id (also its translation key `figure.<id>`) and its strokes, written as
/// `part:x,y x,y …` with strokes separated by `;`. The part names the piece of the figure the
/// stroke draws (translation keys `part.<part>.at` and `part.<part>.lost`), so the reveal can say
/// "on the beak"; a figure piece made of several parts is split into one stroke per part.
pub struct Figure {
    pub id: &'static str,
    strokes: &'static str,
}

/// Number of figures in the deck.
pub const SIZE: usize = 60;

pub static DECK: [Figure; SIZE] = [
    fig(
        "heron",
        "head:18,22 30,14 36,22;neck:36,22 30,36 42,52;body:42,52 64,56 82,50;foot:44,54 44,88;foot:58,57 60,88;beak:30,14 12,18",
    ),
    fig(
        "two-moths",
        "wing:8,40 22,22 30,40 22,62 8,52 22,46;antenna:30,40 34,30;wing:62,36 78,20 86,40 78,62 64,54 76,46;antenna:86,40 90,30",
    ),
    fig(
        "kettle",
        "body:30,32 70,32 76,72 24,72 30,32;spout:70,42 88,36 92,24;handle:30,44 18,52 26,62;lid:44,32 50,20 56,32",
    ),
    fig(
        "lantern",
        "handle:50,8 50,20;glass:36,20 64,20 70,40 64,80 36,80 30,40 36,20;base:40,90 60,90;base:36,80 40,90;base:64,80 60,90",
    ),
    fig(
        "anchor",
        "shaft:50,10 50,82;crossbar:36,24 64,24;arm:20,62 30,80 50,88 70,80 80,62;ring:44,10 50,4 56,10",
    ),
    fig(
        "fox",
        "ear:20,30 28,10 38,28;head:38,28 62,28;ear:62,28 72,10 80,30;cheek:80,30 72,56 50,74;cheek:50,74 28,56 20,30;snout:50,74 50,62",
    ),
    fig(
        "snail",
        "foot:20,80 80,80 86,72;shell:58,78 66,60 56,42 40,44 36,58 46,66 54,60 50,52;head:20,80 14,60 18,52;antenna:14,60 8,54",
    ),
    fig(
        "teacup",
        "rim:20,36 80,36;cup:80,36 72,66 28,66 20,36;handle:80,42 92,46 88,58 76,58;saucer:14,74 86,74",
    ),
    fig(
        "boot",
        "top:36,10 60,10;shaft:60,10 60,60;toe:60,60 86,66 88,84;sole:88,84 36,84;heel:36,84 36,10;heel:36,70 60,70",
    ),
    fig(
        "key",
        "ring:14,40 24,30 34,40 24,50 14,40;shaft:34,40 88,40;tooth:88,40 88,52;tooth:76,40 76,50",
    ),
    fig(
        "umbrella",
        "canopy:10,44 30,24 50,18 70,24 90,44 70,40 50,44 30,40 10,44;tip:50,18 50,8;handle:50,44 50,82 42,88 36,82",
    ),
    fig(
        "sailboat",
        "mast:50,12 50,70;sail:50,14 80,62 50,62;sail:48,20 22,62 48,62;hull:14,72 86,72 76,86 24,86 14,72",
    ),
    fig(
        "lighthouse",
        "tower:40,90 44,30 56,30 60,90 40,90;roof:40,30 50,16 60,30;tip:50,16 50,8;beam:62,22 84,14;beam:62,26 84,34",
    ),
    fig(
        "bell",
        "top:50,8 50,18;top:34,26 50,18 66,26;side:66,26 70,64 82,74;rim:82,74 18,74;side:18,74 30,64 34,26;clapper:46,74 50,86 54,74",
    ),
    fig(
        "hourglass",
        "top:26,10 74,10;side:74,10 54,48 74,88;base:74,88 26,88;side:26,88 46,48 26,10;sand:36,74 64,74",
    ),
    fig(
        "whale",
        "head:8,54 20,36 46,30;back:46,30 72,38 84,52;belly:84,52 72,66 40,70 18,64 8,54;tail:84,52 94,38 96,58 84,52;spout:36,30 32,16 40,24 46,14",
    ),
    fig(
        "crown",
        "side:12,76 12,30;tip:12,30 30,52 50,18 70,52 88,30;side:88,30 88,76;base:88,76 12,76;base:12,86 88,86",
    ),
    fig(
        "feather",
        "shaft:20,86 80,12;edge:80,12 70,40 50,60 30,72;edge:80,12 54,26 38,48 30,72;edge:40,58 30,52;edge:56,40 46,32",
    ),
    fig(
        "fish",
        "head:10,50 30,32;back:30,32 56,30 72,46 72,54;belly:72,54 56,70 30,68;head:30,68 10,50;tail:72,50 90,34 86,50 90,66 72,50;eye:30,46 34,46",
    ),
    fig(
        "mushroom",
        "cap:12,46 24,24 50,14 76,24 88,46 12,46;stem:40,46 38,86 62,86 60,46",
    ),
    fig(
        "house",
        "roof:16,48 50,14 84,48;wall:24,42 24,86 76,86 76,42;door:42,86 42,64 58,64 58,86;chimney:64,30 64,16 72,16 72,38",
    ),
    fig(
        "crescent",
        "tip:62,10 44,20;edge:44,20 36,40 40,62 56,78;tip:56,78 78,82 60,70;edge:60,70 52,52 54,30 62,10",
    ),
    fig(
        "spiral-shell",
        "centre:50,50 56,46 58,54 48,60 40,50;curl:40,50 46,38 62,36 70,52 62,68;edge:62,68 42,72 28,58 30,36 48,24 72,26 84,46",
    ),
    fig(
        "lightning",
        "top:56,6 30,50 50,50;tip:50,50 36,94 74,38;top:74,38 52,38 66,6",
    ),
    fig(
        "pine",
        "top:50,6 30,34;branch:30,34 42,34 22,60 40,60 14,86;base:14,86 86,86;branch:86,86 60,60 78,60 58,34 70,34;top:70,34 50,6;trunk:46,86 46,96 54,96 54,86",
    ),
    fig(
        "cat",
        "body:26,90 22,60 30,40;ear:30,40 26,18 38,30;head:38,30 54,30;ear:54,30 66,18 62,40;body:62,40 70,60 66,90;tail:66,80 84,70 88,52",
    ),
    fig(
        "rabbit",
        "body:30,86 26,64 34,48 50,46 64,54 70,72 62,86 30,86;ear:50,46 46,18 40,6 52,14 56,44;ear:56,44 62,16 70,8 68,30 60,46",
    ),
    fig(
        "swallow",
        "wing:8,30 30,42;body:30,42 50,40 70,30;wing:70,30 92,20;body:30,42 46,52 58,50 70,30;tail:58,50 68,70 60,90;tail:58,50 80,76",
    ),
    fig(
        "fishhook",
        "shaft:60,6 60,70;bend:60,70 50,84 34,84 26,72;tip:26,72 28,60 36,66",
    ),
    fig(
        "comb",
        "spine:10,30 90,30 90,40 10,40 10,30;tooth:20,40 20,70;tooth:34,40 34,70;tooth:48,40 48,70;tooth:62,40 62,70;tooth:76,40 76,70",
    ),
    fig(
        "ladder",
        "rail:30,6 26,94;rail:70,6 74,94;rung:29,20 71,20;rung:28,40 72,40;rung:28,60 73,60;rung:27,80 74,80",
    ),
    fig(
        "candle",
        "body:36,40 64,40 64,92 36,92 36,40;wick:50,40 50,30;flame:50,30 44,20 50,6 56,20 50,30;drip:64,52 70,58 70,66",
    ),
    fig(
        "windmill",
        "tower:40,92 44,46 56,46 60,92 40,92;sail:50,40 22,12;sail:50,40 84,18;sail:50,40 78,70;sail:50,40 18,64",
    ),
    fig(
        "kite",
        "edge:50,6 80,38 50,80 20,38 50,6;cross:20,38 80,38;cross:50,6 50,80;tail:50,80 42,88 56,92 46,98",
    ),
    fig(
        "arrow",
        "shaft:10,90 82,18;tip:82,18 64,20;tip:82,18 80,36;feather:10,90 8,74;feather:10,90 26,92;feather:20,80 18,68;feather:20,80 32,82",
    ),
    fig(
        "harp",
        "pillar:24,90 24,20;neck:24,20 40,10 60,16 76,36;side:76,36 80,90;base:80,90 24,90;string:36,14 36,90;string:50,13 50,90;string:64,22 64,90",
    ),
    fig(
        "horn",
        "mouthpiece:10,44 30,46;body:30,46 52,40 74,22 84,14;bell:84,14 86,40 84,70 74,62;body:74,62 52,52 30,54;mouthpiece:30,54 10,56 10,44",
    ),
    fig(
        "spoon",
        "bowl:26,10 42,10 50,24 44,38 34,40 24,36 18,22 26,10;handle:34,40 70,90 78,88 40,38",
    ),
    fig(
        "scissors",
        "handle:20,20 30,14 40,22 34,32 22,30 20,20;handle:20,80 30,86 40,78 34,68 22,70 20,80;blade:36,30 90,62;blade:36,70 90,38",
    ),
    fig(
        "bridge",
        "deck:4,70 96,70;arch:4,70 20,48 50,36 80,48 96,70;pillar:20,48 20,86;pillar:80,48 80,86;pillar:50,36 50,70",
    ),
    fig(
        "two-peaks",
        "peak:4,86 28,30 42,56;peak:42,56 60,16 96,86;base:96,86 4,86;snow:54,28 60,16 66,28",
    ),
    fig(
        "wave",
        "back:6,70 18,40 34,28;crest:34,28 52,34 58,48 48,56 40,48;trough:58,48 74,62 94,60",
    ),
    fig(
        "frog",
        "body:20,56 30,36 50,32 70,36 80,56 70,70 30,70 20,56;eye:34,34 30,22 40,24;eye:66,34 70,22 60,24;leg:30,70 14,86;leg:70,70 86,86",
    ),
    fig(
        "turtle",
        "shell:20,58 30,34 50,26 70,34 80,58 20,58;head:80,52 92,46 94,56 84,60;leg:28,58 22,72;leg:72,58 78,72;tail:20,58 8,62",
    ),
    fig(
        "owl",
        "ear:30,20 40,30;head:40,30 60,30;ear:60,30 70,20;body:70,20 68,44 74,70 50,90 26,70 32,44 30,20;eye:42,42 44,44;eye:58,42 56,44;beak:46,52 50,58 54,52",
    ),
    fig(
        "dragonfly",
        "body:50,10 50,90;wing:50,28 18,18 10,28 50,34;wing:50,28 82,18 90,28 50,34;wing:50,38 22,46 18,54 50,44;wing:50,38 78,46 82,54 50,44",
    ),
    fig(
        "goblet",
        "rim:22,10 78,10;bowl:78,10 70,36 50,48 30,36 22,10;stem:50,48 50,80;base:32,88 68,88 50,80 32,88",
    ),
    fig(
        "axe",
        "handle:64,8 26,92;blade:54,20 74,8 88,18 92,40 76,46 62,34",
    ),
    fig(
        "flag",
        "pole:24,94 24,8;cloth:24,10 50,18 66,10 86,16 86,44 66,38 50,46 24,40",
    ),
    fig(
        "heart",
        "tip:50,88 16,52;curve:16,52 12,30 24,16 40,18 50,32;curve:50,32 60,18 76,16 88,30 84,52;tip:84,52 50,88",
    ),
    fig(
        "lamp-post",
        "pole:50,94 50,30;lamp:50,30 38,20 38,10 62,10 62,20 50,30;base:36,94 64,94;arm:50,48 70,48 74,56",
    ),
    fig(
        "crab",
        "body:26,50 38,40 62,40 74,50 62,62 38,62 26,50;claw:26,46 12,30 20,22;claw:74,46 88,30 80,22;leg:34,62 24,80;leg:44,62 40,82;leg:56,62 60,82;leg:66,62 76,80",
    ),
    fig(
        "snake",
        "tail:8,80 26,62;body:26,62 46,72 64,54 50,36;head:50,36 64,20 84,22;tongue:84,22 92,14;tongue:84,22 90,30",
    ),
    fig(
        "ribbon-bow",
        "loop:50,48 18,28 14,62 50,48;loop:50,48 86,28 90,62 50,48;end:50,48 36,88;end:50,48 64,88",
    ),
    fig(
        "thimble",
        "side:28,86 32,26;top:32,26 42,14 58,14 68,26;side:68,26 72,86;rim:72,86 28,86;band:30,50 70,50;band:30,70 70,70",
    ),
    fig(
        "broom",
        "handle:70,6 44,58;bristles:44,58 20,90;bristles:44,58 34,94;bristles:44,58 50,96;bristles:44,58 60,92;band:40,64 52,70",
    ),
    fig(
        "giraffe",
        "head:70,8 76,16 66,22;neck:66,22 58,64;body:58,64 34,62;leg:34,62 28,92;leg:58,64 62,92;tail:34,62 22,40;horn:70,8 66,2",
    ),
    fig(
        "rocking-chair",
        "backrest:30,10 34,62;seat:34,62 70,62;leg:70,62 72,82;backrest:30,10 40,10 44,58;leg:34,62 30,82;rocker:14,80 30,90 70,92 90,82",
    ),
    fig(
        "bottle",
        "neck:42,8 58,8 58,28;body:58,28 72,42 72,90;base:72,90 28,90;body:28,90 28,42 42,28;neck:42,28 42,8;label:28,58 72,58;label:28,76 72,76",
    ),
    fig(
        "seesaw",
        "plank:6,40 94,64;base:50,52 36,88 64,88 50,52;seat:10,30 18,30;seat:80,52 90,52",
    ),
];

const fn fig(id: &'static str, strokes: &'static str) -> Figure {
    Figure { id, strokes }
}

/// A line piece: start, end, length in scaled units, and the part it belongs to.
type Segment = ((i64, i64), (i64, i64), i64, &'static str);

/// Grid units are scaled to loom-sized integers before sampling, for precision.
const SCALE: i64 = 1024;

impl Figure {
    /// The strokes as point lists in grid units (0–100).
    pub fn lines(&self) -> Vec<Vec<(i32, i32)>> {
        self.strokes()
            .map(|(_, stroke)| {
                stroke
                    .split_whitespace()
                    .filter_map(|pair| {
                        let (x, y) = pair.split_once(',')?;
                        Some((x.parse().ok()?, y.parse().ok()?))
                    })
                    .collect()
            })
            .collect()
    }

    /// The strokes with their part names.
    fn strokes(&self) -> impl Iterator<Item = (&'static str, &'static str)> {
        self.strokes
            .split(';')
            .map(|stroke| stroke.split_once(':').unwrap_or(("", stroke)))
    }

    /// The part names of the figure, in stroke order (with repeats).
    #[cfg(test)]
    pub fn part_names(&self) -> impl Iterator<Item = &'static str> {
        self.strokes().map(|(part, _)| part)
    }

    /// `n` points at equal distances along all lines, in grid units × 16 (0–1600). Integer
    /// maths, so every client gets the same points.
    pub fn sample(&self, n: usize) -> Vec<Point> {
        self.walk(n).into_iter().map(|(p, _)| p).collect()
    }

    /// The part each of the `n` points of [`Figure::sample`] lies on.
    pub fn parts(&self, n: usize) -> Vec<&'static str> {
        self.walk(n).into_iter().map(|(_, part)| part).collect()
    }

    fn walk(&self, n: usize) -> Vec<(Point, &'static str)> {
        let segments: Vec<Segment> = self
            .strokes()
            .zip(self.lines())
            .flat_map(|((part, _), line)| {
                line.windows(2)
                    .map(|w| {
                        let a = (i64::from(w[0].0) * SCALE, i64::from(w[0].1) * SCALE);
                        let b = (i64::from(w[1].0) * SCALE, i64::from(w[1].1) * SCALE);
                        let len = isqrt(((b.0 - a.0).pow(2) + (b.1 - a.1).pow(2)) as u64) as i64;
                        (a, b, len, part)
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|s| s.2 > 0)
            .collect();
        let total: i64 = segments.iter().map(|s| s.2).sum();
        if n == 0 || total == 0 {
            return Vec::new();
        }
        let n_i = n as i64;
        let mut points = Vec::with_capacity(n);
        let mut seg = 0;
        let mut start = 0i64;
        for k in 0..n_i {
            // The middle of the k-th of n equal parts, so the ends are not favoured.
            let at = (2 * k + 1) * total / (2 * n_i);
            while seg + 1 < segments.len() && start + segments[seg].2 <= at {
                start += segments[seg].2;
                seg += 1;
            }
            let (a, b, len, part) = segments[seg];
            let along = (at - start).min(len);
            let x = a.0 + (b.0 - a.0) * along / len;
            let y = a.1 + (b.1 - a.1) * along / len;
            points.push((((x * 16 / SCALE) as i32, (y * 16 / SCALE) as i32), part));
        }
        points
    }
}

/// The figure with this deck index.
pub fn get(index: usize) -> Option<&'static Figure> {
    DECK.get(index)
}
