//! Deterministic visual effects. Every function is pure in its inputs (frame
//! index, sizes, seed); nothing reads a clock, so snapshots are stable and a
//! frozen frame is just a fixed argument.
use super::theme::{Tone, char_width, glyph};

/// Small integer hash used for dithering and scattered accents.
pub fn hash(x: u32, y: u32, seed: u32) -> u32 {
    let mut h = x
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(y.wrapping_mul(0x85EB_CA77))
        .wrapping_add(seed.wrapping_mul(0xC2B2_AE3D));
    h ^= h >> 15;
    h = h.wrapping_mul(0x27D4_EB2F);
    h ^= h >> 13;
    h
}

fn unit(x: u32, y: u32, seed: u32) -> f32 {
    (hash(x, y, seed) % 1000) as f32 / 1000.0
}

/// A dotted rule that thins out toward its right end.
pub fn fade_rule(n: usize) -> String {
    (0..n)
        .map(|i| {
            let t = i as f32 / n.max(1) as f32;
            if t < 0.6 || unit(i as u32, 7, 3) > (t - 0.6) * 2.4 {
                '·'
            } else {
                ' '
            }
        })
        .collect()
}

pub fn spinner(frame: usize) -> char {
    glyph::SPINNER[frame % glyph::SPINNER.len()]
}

/// Smooth bar: full blocks, one eighth block, then a dotted track.
pub fn bar(fraction: f64, cols: usize) -> (String, String) {
    let fraction = fraction.clamp(0.0, 1.0);
    let eighths = (fraction * cols as f64 * 8.0).round() as usize;
    let full = eighths / 8;
    let part = eighths % 8;
    let mut done = "█".repeat(full.min(cols));
    if part > 0 && full < cols {
        done.push(glyph::BAR[part - 1]);
    }
    let used = full + usize::from(part > 0 && full < cols);
    (done, "·".repeat(cols.saturating_sub(used)))
}

/// Noise for the scramble reveal. From ascii.rest `scramble` (MIT, © 2026
/// bas3line); see `THIRD_PARTY.md`.
const NOISE: [char; 20] = [
    '!', '<', '>', '-', '_', '/', '[', ']', '{', '}', '=', '+', '*', '^', '?', '#', '%', '&', '$',
    '@',
];

/// Scramble reveal: characters left of `progress` are resolved, the rest are
/// noise glyphs of equal display width. `progress >= 1` returns `text`.
pub fn scramble(text: &str, progress: f32, seed: u32) -> String {
    let total = text.chars().count();
    let shown = ((progress.clamp(0.0, 1.0)) * total as f32).round() as usize;
    text.chars()
        .enumerate()
        .map(|(i, c)| {
            if i < shown || c == ' ' {
                c.to_string()
            } else {
                let w = char_width(c).max(1);
                (0..w)
                    .map(|k| NOISE[hash(i as u32, k as u32, seed) as usize % NOISE.len()])
                    .collect()
            }
        })
        .collect()
}

/// Cubic ease-out on 0..1.
pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Progress of a one-shot animation that runs `frames` frames; the resolved
/// still frame (`usize::MAX`, or anything past the end) is 1.
pub fn progress(frame: usize, frames: usize) -> f32 {
    if frames <= 1 || frame >= frames - 1 {
        1.0
    } else {
        frame as f32 / (frames - 1) as f32
    }
}

/// Skeleton shimmer: the column a three-cell light band sits on while it
/// travels over `cols` filled cells, or `None` when frozen or nothing is
/// filled. One pass takes `cols + 6` frames, then it rests for 6.
pub fn shimmer(cols: usize, frame: usize) -> Option<usize> {
    if cols == 0 || frame == usize::MAX {
        return None;
    }
    let period = cols + 12;
    let at = frame % period;
    (at < cols + 3).then(|| at.saturating_sub(1))
}

/// Sparkline over `values`, right-aligned in `cols` cells (latest at the
/// right), scaled to the largest value. Zero is the lowest bar.
pub fn spark(values: &[u64], cols: usize) -> String {
    let tail = &values[values.len().saturating_sub(cols)..];
    let max = tail.iter().copied().max().unwrap_or(0).max(1);
    let mut out = " ".repeat(cols.saturating_sub(tail.len()));
    for &v in tail {
        let level = (v * 7).div_ceil(max) as usize;
        out.push(glyph::SPARK[level.min(7)]);
    }
    out
}

/// Five-cell confidence meter, e.g. `▰▰▰▰▱` for 0.8.
pub fn pips(value: f64) -> (String, String) {
    let on = ((value.clamp(0.0, 1.0) * 5.0).round() as usize).min(5);
    ("▰".repeat(on), "▱".repeat(5 - on))
}

/// Evidence bar with a threshold tick: cells left of `value` are filled,
/// the tick `┊` marks `threshold`. Returns `(cell, filled)` pairs.
pub fn evidence_bar(value: f64, threshold: f64, cols: usize) -> Vec<(char, bool)> {
    let fill = (value.clamp(0.0, 1.0) * cols as f64).round() as usize;
    let tick = ((threshold.clamp(0.0, 1.0) * cols as f64).round() as usize).min(cols - 1);
    (0..cols)
        .map(|i| {
            if i == tick && i >= fill {
                ('┊', false)
            } else if i < fill {
                ('█', true)
            } else {
                ('·', false)
            }
        })
        .collect()
}

/// Log-scale magnitude as a four-level stack (`▁▃▅▇`), for row counts.
pub fn magnitude_bar(rows: Option<u64>) -> char {
    match rows {
        None | Some(0) => ' ',
        Some(n) if n < 1_000 => '▁',
        Some(n) if n < 100_000 => '▃',
        Some(n) if n < 1_000_000 => '▅',
        Some(_) => '▇',
    }
}

/// One splash cell.
pub type Cell = (char, Tone);

pub const SPLASH_FRAMES: usize = 16;
pub const SPLASH_FRAME_MS: u64 = 70;

/// Pixel font for the wordmark: five rows a letter, `#` ink. From ascii.rest
/// `big-text` (MIT, © 2026 bas3line), only the letters `OPEN ONTOLOGY` uses.
fn font(c: char) -> Option<[&'static str; 5]> {
    Some(match c {
        'E' => ["####", "#...", "###.", "#...", "####"],
        'G' => [".###", "#...", "#.##", "#..#", ".###"],
        'L' => ["#...", "#...", "#...", "#...", "####"],
        'N' => ["#...#", "##..#", "#.#.#", "#..##", "#...#"],
        'O' => [".##.", "#..#", "#..#", "#..#", ".##."],
        'P' => ["###.", "#..#", "###.", "#...", "#..."],
        'T' => ["#####", "..#..", "..#..", "..#..", "..#.."],
        'Y' => ["#...#", ".#.#.", "..#..", "..#..", "..#.."],
        ' ' => ["..", "..", "..", "..", ".."],
        _ => return None,
    })
}

/// Shadow cell by which outline edges meet at its top-left corner: up 1,
/// down 2, left 4, right 8. From ascii.rest `big-text` (MIT, © 2026 bas3line).
const JOIN: [char; 16] = [
    ' ', '║', '║', '║', '═', '╝', '╗', '╣', '═', '╚', '╔', '╠', '═', '╩', '╦', '╬',
];

/// A block-letter word with its line shadow.
pub struct Wordmark {
    pub cols: usize,
    pub rows: usize,
    /// `true` for ink cells.
    pub ink: Vec<Vec<bool>>,
    /// Shadow glyph for non-ink cells (' ' for none).
    pub shadow: Vec<Vec<char>>,
    /// Column where the second word starts (for two-tone colouring).
    pub split: usize,
}

/// Lay out `text` (letters in `font`) one column a pixel, one blank column
/// between letters, plus one row and column of shadow.
pub fn wordmark(text: &str) -> Wordmark {
    let glyphs: Vec<[&str; 5]> = text.chars().filter_map(font).collect();
    let cols = glyphs.iter().map(|g| g[0].len() + 1).sum::<usize>() + 1;
    let rows = 6;
    let mut ink = vec![vec![false; cols]; rows];
    let mut x = 0;
    let mut split = 0;
    for (k, g) in glyphs.iter().enumerate() {
        if g[0] == ".." && split == 0 && k > 0 {
            split = x;
        }
        for (y, line) in g.iter().enumerate() {
            for (i, c) in line.chars().enumerate() {
                if c == '#' {
                    ink[y][x + i] = true;
                }
            }
        }
        x += g[0].len() + 1;
    }
    let at = |c: isize, r: isize| -> bool {
        c >= 0
            && r >= 0
            && (r as usize) < rows
            && (c as usize) < cols
            && ink[r as usize][c as usize]
    };
    let mut shadow = vec![vec![' '; cols]; rows];
    for (r, row) in shadow.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            let (ci, ri) = (c as isize, r as isize);
            if at(ci, ri) {
                continue;
            }
            let (a, b, d) = (at(ci - 1, ri - 1), at(ci, ri - 1), at(ci - 1, ri));
            let edges = usize::from(a != b)
                | (usize::from(d) << 1)
                | (usize::from(a != d) << 2)
                | (usize::from(b) << 3);
            *cell = JOIN[if edges == 15 { 2 | 8 } else { edges }];
        }
    }
    Wordmark {
        cols,
        rows,
        ink,
        shadow,
        split,
    }
}

/// Distance of cell `(x, y)` from a slanted glint band at `at`; the band
/// core is `< 1.2`, its edge `< 2.4`. Formula after ascii.rest kit `glint`
/// (MIT, © 2026 bas3line).
pub fn glint_distance(x: usize, y: usize, at: f32) -> f32 {
    (x as f32 + 1.2 * y as f32 - at).abs()
}

/// Dust ramp a gathering cell walks up, after ascii.rest `dissolve`.
const DUST: [char; 6] = ['.', '·', '░', '▒', '▓', '█'];

/// The splash: dust rising out of the data condenses into the block-letter
/// wordmark (left to right, each cell a little late), a glint crosses it,
/// and the tagline decodes. `frame >= SPLASH_FRAMES - 1` is the resolved
/// still frame used for reduced motion, `NO_COLOR` and snapshots.
pub fn splash(w: usize, h: usize, frame: usize, zh: bool) -> Vec<Vec<Cell>> {
    let mut grid = vec![vec![(' ', Tone::Faint); w]; h];
    if w < 20 || h < 8 {
        return grid;
    }
    let p = progress(frame, SPLASH_FRAMES);
    let tag = if zh {
        "数据库  ─▶  本体  ─▶  Agent"
    } else {
        "database  ─▶  ontology  ─▶  agent"
    };
    let mark = wordmark("OPEN ONTOLOGY");
    let big = w >= mark.cols + 4 && h >= 14;
    let block_h = if big { mark.rows + 3 } else { 3 };
    let y0 = (h.saturating_sub(block_h)) / 2;
    starfield(&mut grid, y0, block_h, p);
    if big {
        let x0 = (w - mark.cols) / 2;
        let settle = 0.62;
        let glint = if p > settle && p < 1.0 {
            Some(-3.0 + (mark.cols as f32 + 12.0) * (p - settle) / (1.0 - settle))
        } else {
            None
        };
        for y in 0..mark.rows {
            for x in 0..mark.cols {
                if mark.ink[y][x] {
                    let delay =
                        0.42 * x as f32 / mark.cols as f32 + 0.08 * unit(x as u32, y as u32, 23);
                    let u = ((p - delay) / 0.3).clamp(0.0, 1.0);
                    if u <= 0.0 {
                        continue;
                    }
                    let e = ease_out(u);
                    let rise =
                        ((1.0 - e) * (2.0 + 2.0 * unit(x as u32, y as u32, 29))).round() as usize;
                    let (gx, gy) = (x0 + x, y0 + y + rise);
                    if gy >= h {
                        continue;
                    }
                    let level =
                        ((e * (DUST.len() - 1) as f32).round() as usize).min(DUST.len() - 1);
                    let word_tone = if x >= mark.split {
                        Tone::Accent
                    } else {
                        Tone::Ink
                    };
                    let mut cell = if level + 1 < DUST.len() {
                        (
                            DUST[level],
                            if level < 2 { Tone::Faint } else { Tone::Ember },
                        )
                    } else {
                        ('█', word_tone)
                    };
                    if let Some(at) = glint
                        && cell.0 == '█'
                    {
                        let d = glint_distance(x, y, at);
                        if d < 1.2 {
                            cell = ('▓', Tone::Hot);
                        } else if d < 2.4 {
                            cell = ('█', Tone::Hot);
                        }
                    }
                    grid[gy][gx] = cell;
                } else if p >= settle && mark.shadow[y][x] != ' ' {
                    grid[y0 + y][x0 + x] = (mark.shadow[y][x], Tone::Ember);
                }
            }
        }
    } else {
        let word = scramble(
            "o p e n   o n t o l o g y",
            (p * 1.6).min(1.0),
            frame as u32,
        );
        put(&mut grid, y0, &word, Tone::Ink);
    }
    let tag_p = ((p - 0.45) / 0.45).clamp(0.0, 1.0);
    if tag_p > 0.0 {
        let shown = scramble(tag, tag_p, frame as u32 ^ 0x51);
        put(&mut grid, y0 + block_h - 1, &shown, Tone::Dim);
        // The arrows carry the accent once the line has settled.
        if tag_p >= 1.0 {
            let y = y0 + block_h - 1;
            for cell in grid[y].iter_mut() {
                if cell.0 == '▶' || cell.0 == '─' {
                    cell.1 = Tone::Ember;
                }
            }
        }
    }
    grid
}

/// Sparse dots that thin out away from the wordmark band, densest close to
/// it; fades in with the splash. Only the splash and heading textures carry
/// decoration.
fn starfield(grid: &mut [Vec<Cell>], band_y: usize, band_h: usize, p: f32) {
    let h = grid.len();
    let w = grid[0].len();
    let cy = band_y as f32 + band_h as f32 / 2.0;
    for (y, row) in grid.iter_mut().enumerate() {
        if y + 1 >= band_y && y <= band_y + band_h {
            continue;
        }
        let dy = (y as f32 - cy).abs() / (h as f32 / 2.0);
        for (x, cell) in row.iter_mut().enumerate() {
            let dx = (x as f32 - w as f32 / 2.0).abs() / (w as f32 / 2.0);
            let near = (1.0 - dy).max(0.0) * (1.0 - 0.6 * dx);
            let u = unit(x as u32, y as u32, 5);
            if u < 0.05 * near * near * (0.3 + 0.7 * p) {
                *cell = if u < 0.0025 {
                    ('✦', Tone::Ember)
                } else if u < 0.01 {
                    ('•', Tone::Faint)
                } else {
                    ('·', Tone::Faint)
                };
            }
        }
    }
}

/// One burst particle: offset from the origin, glyph, tone.
pub type Particle = (i32, i32, char, Tone);

pub const BURST_FRAMES: usize = 10;

/// A one-shot burst (after ascii.rest `fireworks` peonies): sixteen sparks
/// fly out from the origin, slow down, sag and cool from `✦` to `·`, then
/// vanish. Empty at and after `BURST_FRAMES` (and for the still frame).
pub fn burst(frame: usize) -> Vec<Particle> {
    if frame >= BURST_FRAMES {
        return Vec::new();
    }
    let u = (frame + 1) as f32 / BURST_FRAMES as f32;
    let e = ease_out(u);
    (0..16)
        .map(|k| {
            let a = std::f32::consts::TAU * (k as f32 + 0.3 * unit(k, 1, 41)) / 16.0;
            let speed = 3.0 + 2.5 * unit(k, 2, 43);
            let x = (a.cos() * speed * e * 2.0).round() as i32;
            let y = (a.sin() * speed * e + 1.5 * u * u).round() as i32;
            let (c, tone) = if u < 0.3 {
                ('✦', Tone::Hot)
            } else if u < 0.55 {
                ('+', Tone::Accent)
            } else if u < 0.8 {
                ('•', Tone::Ember)
            } else {
                ('·', Tone::Faint)
            };
            (x, y, c, tone)
        })
        .collect()
}

fn put(grid: &mut [Vec<Cell>], y: usize, text: &str, tone: Tone) {
    let w = grid[y].len();
    let tw = super::theme::width(text);
    let mut x = w.saturating_sub(tw) / 2;
    for c in text.chars() {
        let cw = char_width(c);
        if x + cw > w {
            break;
        }
        grid[y][x] = (c, if c == ' ' { Tone::Faint } else { tone });
        for k in 1..cw {
            grid[y][x + k] = ('\0', tone);
        }
        x += cw;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_are_deterministic_and_width_preserving() {
        assert_eq!(
            splash(80, 24, 99, false),
            splash(80, 24, SPLASH_FRAMES - 1, false)
        );
        assert_ne!(
            splash(80, 24, 0, false),
            splash(80, 24, SPLASH_FRAMES - 1, false)
        );
        assert_eq!(
            splash(80, 24, usize::MAX, true),
            splash(80, 24, SPLASH_FRAMES, true)
        );
        let s = scramble("订单 order", 0.0, 1);
        assert_eq!(
            super::super::theme::width(&s),
            super::super::theme::width("订单 order")
        );
        assert_eq!(scramble("订单", 1.0, 3), "订单");
        let (done, rest) = bar(0.5, 10);
        assert_eq!(done.chars().count() + rest.chars().count(), 10);
        assert_eq!(bar(1.0, 4).0, "████");
        assert_eq!(fade_rule(30).chars().count(), 30);
        assert_eq!(spark(&[0, 1, 2, 4], 6), "  ▁▃▅█");
        assert_eq!(pips(0.8), ("▰▰▰▰".to_owned(), "▱".to_owned()));
        assert!(burst(BURST_FRAMES).is_empty() && burst(usize::MAX).is_empty());
        assert_eq!(shimmer(10, usize::MAX), None);
    }

    #[test]
    fn wordmark_fits_eighty_columns_with_its_shadow() {
        let mark = wordmark("OPEN ONTOLOGY");
        assert!(mark.cols <= 76, "{}", mark.cols);
        assert_eq!(mark.rows, 6);
        // The shadow sits right of and below the ink.
        assert_eq!(mark.shadow[0][3], '╗');
        assert!(mark.split > 20 && mark.split < 30);
    }
}
