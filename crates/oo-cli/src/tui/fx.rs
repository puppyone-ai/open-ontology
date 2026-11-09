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

/// Map brightness 0..1 to the dot ramp with interleaved-row dithering: odd
/// rows shift the thresholds by half a step, which reads as a smoother
/// gradient than a plain quantiser.
pub fn ramp(v: f32, x: u32, y: u32) -> char {
    let v = v.clamp(0.0, 1.0);
    let offset = if (y + x / 2) % 2 == 1 { 0.5 } else { 0.0 };
    let steps = (glyph::RAMP.len() - 1) as f32;
    let index = (v * steps + offset * 0.5).floor() as usize;
    glyph::RAMP[index.min(glyph::RAMP.len() - 1)]
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

/// Scramble reveal: characters left of `progress` are resolved, the rest are
/// noise glyphs of equal display width. `progress >= 1` returns `text`.
pub fn scramble(text: &str, progress: f32, seed: u32) -> String {
    let total = text.chars().count();
    let shown = ((progress.clamp(0.0, 1.0)) * total as f32).round() as usize;
    const NOISE: [char; 6] = ['·', '•', '░', '▒', '/', '+'];
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

/// Typewriter: the first `n` characters.
pub fn typewriter(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
}

/// One splash cell.
pub type Cell = (char, Tone);

pub const SPLASH_FRAMES: usize = 18;
pub const SPLASH_FRAME_MS: u64 = 80;

/// The splash: a dot-shaded orb whose light sweeps in, scattered accent
/// glyphs fading out toward the edges, and the wordmark scrambling into
/// place. `frame >= SPLASH_FRAMES - 1` is the resolved still frame used for
/// reduced motion, `NO_COLOR` and snapshots.
pub fn splash(w: usize, h: usize, frame: usize) -> Vec<Vec<Cell>> {
    let mut grid = vec![vec![(' ', Tone::Faint); w]; h];
    if w < 20 || h < 8 {
        return grid;
    }
    let t = (frame.min(SPLASH_FRAMES - 1)) as f32 / (SPLASH_FRAMES - 1) as f32;
    let cx = w as f32 / 2.0;
    let cy = (h as f32 - 4.0) / 2.0;
    let r = (h as f32 - 6.0).min(w as f32 / 4.0) / 2.0 * 1.1;
    // Light swings from behind-left to upper-left as the splash resolves.
    let angle = (1.0 - t) * 2.2 + 0.7;
    let (lx, ly, lz) = (-angle.cos() * 0.8, -0.55, angle.sin() * 0.6 + 0.2);
    let norm = (lx * lx + ly * ly + lz * lz).sqrt();
    for (y, row) in grid.iter_mut().enumerate() {
        for (x, cell) in row.iter_mut().enumerate() {
            let dx = (x as f32 - cx) / 2.0 / r;
            let dy = (y as f32 - cy) / r;
            let d2 = dx * dx + dy * dy;
            if d2 <= 1.0 {
                let dz = (1.0 - d2).sqrt();
                let lambert = ((dx * lx + dy * ly + dz * lz) / norm).max(0.0);
                let v = 0.08 + 0.92 * lambert;
                // Sparsity fade at the rim.
                if d2 > 0.82 && unit(x as u32, y as u32, 11) < (d2 - 0.82) * 4.0 {
                    continue;
                }
                let c = ramp(v, x as u32, y as u32);
                let tone = if v > 0.8 {
                    Tone::Accent
                } else if v > 0.45 {
                    Tone::Ink
                } else {
                    Tone::Dim
                };
                *cell = (c, tone);
            } else {
                // Scattered accents, thinning with distance from the orb.
                let falloff = (1.0 / d2).min(1.0);
                let u = unit(x as u32, y as u32, 5);
                if u < 0.018 * falloff * (0.4 + t) {
                    let g = glyph::ACCENTS[hash(x as u32, y as u32, 9) as usize % 4];
                    let tone = if u < 0.004 { Tone::Accent } else { Tone::Faint };
                    *cell = (g, tone);
                } else if u < 0.06 * falloff * falloff {
                    *cell = ('·', Tone::Faint);
                }
            }
        }
    }
    let word = scramble(
        "o p e n   o n t o l o g y",
        (t * 1.6).min(1.0),
        frame as u32,
    );
    let tag = "database → ontology → agent";
    let tag_shown = typewriter(
        tag,
        ((t - 0.4).max(0.0) / 0.6 * tag.chars().count() as f32) as usize,
    );
    put(&mut grid, h - 3, &word, Tone::Ink);
    put(
        &mut grid,
        h - 2,
        &format!("{:<width$}", tag_shown, width = tag.chars().count()),
        Tone::Dim,
    );
    grid
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
        grid[y][x] = (c, tone);
        for k in 1..cw {
            grid[y][x + k] = ('\0', tone);
        }
        x += cw;
    }
    for cell in &mut grid[y] {
        if cell.0 == ' ' {
            cell.1 = Tone::Faint;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_are_deterministic_and_width_preserving() {
        assert_eq!(splash(80, 24, 99), splash(80, 24, SPLASH_FRAMES - 1));
        assert_ne!(splash(80, 24, 0), splash(80, 24, SPLASH_FRAMES - 1));
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
    }
}
