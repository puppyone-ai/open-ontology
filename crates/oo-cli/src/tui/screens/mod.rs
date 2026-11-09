//! The five screens. Each is `State` + `render(&State, Rect, &mut Buffer,
//! &Theme)` + `key(&mut State, KeyEvent) -> Action`; none touches the terminal.
pub mod build;
pub mod confirm;
pub mod connect;
pub mod deliver;
pub mod select;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use serde::Serialize;

use super::fx;
use super::theme::{Theme, Tone, fit, width};

/// What a key press asks the runner to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    None,
    /// Leave the screen and go on.
    Next,
    /// Leave without completing (Esc / q / Ctrl-C).
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Waiting,
    Running,
    Pass,
    Warn,
    Fail,
}

/// One live checklist row (doctor, readiness).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Check {
    pub name: String,
    pub status: Status,
    pub detail: String,
    /// Fix text shown under a failed or warned check. Advice only; never run.
    pub fix: String,
}

impl Check {
    pub fn new(name: &str, status: Status, detail: &str) -> Self {
        Check {
            name: name.into(),
            status,
            detail: detail.into(),
            fix: String::new(),
        }
    }

    pub fn fix(mut self, fix: &str) -> Self {
        self.fix = fix.into();
        self
    }
}

/// Write spans at (x, y), clipped to `w` columns.
pub fn put(buf: &mut Buffer, x: u16, y: u16, w: u16, spans: Vec<Span<'_>>) {
    if w == 0 || y >= buf.area.bottom() {
        return;
    }
    buf.set_line(x, y, &Line::from(spans), w);
}

/// Checklist: status glyph, name, detail; fix text on its own line. Returns
/// rows used.
pub fn checklist(
    buf: &mut Buffer,
    area: Rect,
    theme: &Theme,
    checks: &[Check],
    frame: usize,
) -> u16 {
    let mut y = area.y;
    let name_w = checks
        .iter()
        .map(|c| width(&c.name))
        .max()
        .unwrap_or(0)
        .min(22)
        + 2;
    for check in checks {
        if y >= area.bottom() {
            break;
        }
        let spinner = fx::spinner(frame).to_string();
        let (mark, tone) = match check.status {
            Status::Waiting => ("·", Tone::Faint),
            Status::Running => (spinner.as_str(), Tone::Accent),
            Status::Pass => ("✓", Tone::Ok),
            Status::Warn => ("!", Tone::Warn),
            Status::Fail => ("✗", Tone::Bad),
        };
        let name_tone = if check.status == Status::Waiting {
            Tone::Dim
        } else {
            Tone::Ink
        };
        let detail_w = usize::from(area.width).saturating_sub(name_w + 3);
        put(
            buf,
            area.x,
            y,
            area.width,
            vec![
                theme.span(format!("{mark} "), tone),
                theme.span(super::theme::pad(&check.name, name_w), name_tone),
                theme.span(fit(&check.detail, detail_w), Tone::Dim),
            ],
        );
        y += 1;
        if !check.fix.is_empty()
            && matches!(check.status, Status::Fail | Status::Warn)
            && y < area.bottom()
        {
            let w = usize::from(area.width).saturating_sub(name_w + 2 + 5);
            put(
                buf,
                area.x + 2 + name_w as u16,
                y,
                area.width.saturating_sub(2 + name_w as u16),
                vec![
                    theme.span("fix ", tone),
                    theme.span(fit(&check.fix, w), Tone::Ink),
                ],
            );
            y += 1;
        }
    }
    y - area.y
}

/// A small text input with a caret.
pub fn input(buf: &mut Buffer, x: u16, y: u16, w: u16, theme: &Theme, shown: &str, active: bool) {
    let room = usize::from(w).saturating_sub(3);
    // Keep the tail visible while typing.
    let mut text = shown.to_owned();
    while width(&text) > room {
        text.remove(0);
    }
    let caret = if active { "▏" } else { "" };
    put(
        buf,
        x,
        y,
        w,
        vec![
            theme.span("› ", if active { Tone::Accent } else { Tone::Faint }),
            theme.span(text, Tone::Ink),
            theme.span(caret, Tone::Accent),
        ],
    );
}

/// Centered modal frame; returns the inner area.
pub fn modal(buf: &mut Buffer, area: Rect, theme: &Theme, title: &str, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2));
    let h = h.min(area.height.saturating_sub(2));
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    let edge = theme.style(Tone::Accent);
    for cy in y..y + h {
        for cx in x..x + w {
            buf[(cx, cy)].reset();
            buf[(cx, cy)].set_symbol(" ");
        }
    }
    for cx in x..x + w {
        buf[(cx, y)].set_symbol("─").set_style(edge);
        buf[(cx, y + h - 1)].set_symbol("─").set_style(edge);
    }
    for cy in y..y + h {
        buf[(x, cy)].set_symbol("│").set_style(edge);
        buf[(x + w - 1, cy)].set_symbol("│").set_style(edge);
    }
    buf[(x, y)].set_symbol("╭").set_style(edge);
    buf[(x + w - 1, y)].set_symbol("╮").set_style(edge);
    buf[(x, y + h - 1)].set_symbol("╰").set_style(edge);
    buf[(x + w - 1, y + h - 1)].set_symbol("╯").set_style(edge);
    put(
        buf,
        x + 2,
        y,
        w.saturating_sub(4),
        vec![theme.span(format!(" {title} "), Tone::Accent)],
    );
    Rect::new(x + 2, y + 1, w.saturating_sub(4), h.saturating_sub(2))
}

/// Paint a pure character canvas (graph, splash) into the buffer.
pub fn blit(
    buf: &mut Buffer,
    area: Rect,
    theme: &Theme,
    cells: &[Vec<(char, Tone)>],
    offset: (usize, usize),
) {
    for (row, line) in cells
        .iter()
        .skip(offset.1)
        .take(usize::from(area.height))
        .enumerate()
    {
        let y = area.y + row as u16;
        let mut col = 0usize;
        for (cx, &(c, tone)) in line.iter().enumerate().skip(offset.0) {
            col = cx - offset.0;
            if col >= usize::from(area.width) {
                break;
            }
            let x = area.x + col as u16;
            if c == '\0' {
                continue;
            }
            let w = super::theme::char_width(c);
            if col + w > usize::from(area.width) {
                break;
            }
            if c != ' ' {
                let mut tmp = [0u8; 4];
                buf[(x, y)]
                    .set_symbol(c.encode_utf8(&mut tmp))
                    .set_style(theme.style(tone));
            }
        }
        let _ = col;
    }
}

/// Ambient dot field in the blank rows at the bottom of `area`: densest in
/// the bottom-right corner, thinning up and to the left, with a rare accent.
/// Deterministic (hash of position), so snapshots stay stable.
pub fn ambient(buf: &mut Buffer, area: Rect, theme: &Theme) {
    let mut band = 0u16;
    while band < 6 && band < area.height {
        let y = area.bottom() - 1 - band;
        if (area.left()..area.right()).any(|x| !buf[(x, y)].symbol().trim().is_empty()) {
            break;
        }
        band += 1;
    }
    if band < 2 {
        return;
    }
    // Leave one blank row between content and the field.
    let rows = band - 1;
    for k in 0..rows {
        let y = area.bottom() - 1 - k;
        let vy = 1.0 - f32::from(k) / f32::from(rows);
        for x in area.left()..area.right() {
            let vx = f32::from(x - area.left()) / f32::from(area.width.max(1));
            let p = 0.22 * vy * vy * vx.powf(1.6);
            let u = (fx::hash(u32::from(x), u32::from(y), 17) % 1000) as f32 / 1000.0;
            if u < p {
                let (c, tone) = if u < p * 0.04 {
                    ("✦", Tone::Accent)
                } else if u < p * 0.25 {
                    ("•", Tone::Faint)
                } else {
                    ("·", Tone::Faint)
                };
                buf[(x, y)].set_symbol(c).set_style(theme.style(tone));
            }
        }
    }
}

#[cfg(test)]
/// Plain-text grid of a buffer (wide glyphs occupy their columns once).
pub fn buffer_text(buf: &Buffer) -> String {
    let mut out = String::new();
    for y in buf.area.top()..buf.area.bottom() {
        let mut line = String::new();
        let mut skip = 0;
        for x in buf.area.left()..buf.area.right() {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let symbol = buf[(x, y)].symbol();
            line.push_str(symbol);
            skip = width(symbol).saturating_sub(1);
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
/// Colour layer: one tone letter per column (`.` blank, `?` unstyled ink).
pub fn buffer_tones(buf: &Buffer, theme: &Theme) -> String {
    let mut out = String::new();
    for y in buf.area.top()..buf.area.bottom() {
        let mut line = String::new();
        for x in buf.area.left()..buf.area.right() {
            let cell = &buf[(x, y)];
            if cell.symbol().trim().is_empty() {
                line.push('.');
                continue;
            }
            let reversed = cell.modifier.contains(ratatui::style::Modifier::REVERSED);
            let code = theme
                .tone_of(cell.style())
                .map_or('?', super::theme::Tone::code);
            line.push(if reversed {
                code.to_ascii_uppercase()
            } else {
                code
            });
        }
        out.push_str(line.trim_end_matches('.'));
        out.push('\n');
    }
    out
}

#[cfg(test)]
pub(crate) mod snapshot {
    use super::*;
    use std::path::PathBuf;

    /// Render into a TestBackend-sized buffer through a real `Terminal`, then
    /// compare against `tests/snapshots/tui/<name>.txt`. `OO_UPDATE_SNAPSHOTS=1`
    /// rewrites the files.
    pub fn check(
        name: &str,
        w: u16,
        h: u16,
        theme: Theme,
        draw: impl Fn(Rect, &mut Buffer, &Theme),
    ) {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw(area, frame.buffer_mut(), &theme);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut text = buffer_text(&buf);
        if theme.color {
            text.push_str("--- colour layer ---\n");
            text.push_str(&buffer_tones(&buf, &theme));
        }
        for line in text.lines() {
            if line.starts_with("--- colour") {
                break;
            }
            assert!(
                width(line) <= usize::from(w),
                "{name}: line wider than {w}: {line}"
            );
        }
        let mode = if theme.color { "color" } else { "mono" };
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots/tui")
            .join(format!("{name}-{w}x{h}-{mode}.txt"));
        if std::env::var_os("OO_UPDATE_SNAPSHOTS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &text).unwrap();
            return;
        }
        let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!(
                "missing snapshot {}; run with OO_UPDATE_SNAPSHOTS=1",
                path.display()
            )
        });
        assert_eq!(text, expected, "snapshot {} differs", path.display());
    }

    /// Every screen at 80×24 and 120×40, mono and colour.
    pub fn all_sizes(name: &str, draw: impl Fn(Rect, &mut Buffer, &Theme)) {
        for (w, h) in [(80, 24), (120, 40)] {
            for theme in [Theme::MONO, Theme::COLOR] {
                check(name, w, h, theme, &draw);
            }
        }
    }
}

/// The splash: dot-shaded orb, scattered accents, wordmark. `frame` past the
/// last index is the resolved still frame.
pub fn splash(frame: usize, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let cells = fx::splash(usize::from(area.width), usize::from(area.height), frame);
    blit(buf, area, theme, &cells, (0, 0));
}

#[cfg(test)]
mod splash_tests {
    use super::*;

    #[test]
    fn splash_snapshots() {
        snapshot::all_sizes("0-splash", |area, buf, theme| {
            splash(usize::MAX, area, buf, theme)
        });
    }
}

/// Themed free-text question for the remaining legacy wizard steps.
#[allow(clippy::too_many_arguments)]
pub fn prompt(
    step: usize,
    label: &str,
    default: &str,
    shown: &str,
    secret: bool,
    scroll: u16,
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
) {
    use ratatui::widgets::{Paragraph, Widget, Wrap};
    let inner = super::theme::frame(
        buf,
        area,
        theme,
        step,
        &[("⏎", "accept"), ("esc", "cancel"), ("↑↓", "scroll")],
    );
    if inner.height < 6 {
        return;
    }
    let mut text = label.to_owned();
    if !secret && !default.is_empty() {
        text.push_str(&format!("\n\ndefault: {default}"));
    }
    Paragraph::new(text)
        .style(theme.style(Tone::Ink))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0))
        .render(
            Rect::new(
                inner.x,
                inner.y + 1,
                inner.width,
                inner.height.saturating_sub(5),
            ),
            buf,
        );
    input(
        buf,
        inner.x,
        inner.bottom() - 3,
        inner.width,
        theme,
        shown,
        true,
    );
    put(
        buf,
        inner.x,
        inner.bottom() - 1,
        inner.width,
        vec![theme.span(
            "credentials already entered stay saved if you cancel",
            Tone::Faint,
        )],
    );
}

#[cfg(test)]
mod prompt_tests {
    use super::*;

    #[test]
    fn prompt_snapshots() {
        snapshot::check("0-prompt", 80, 24, Theme::MONO, |area, buf, theme| {
            prompt(
                2,
                "Model base URL, or @PATH to import a private endpoint file",
                "",
                "@~/private/endpoint",
                false,
                0,
                area,
                buf,
                theme,
            )
        });
        snapshot::check("0-prompt", 80, 24, Theme::COLOR, |area, buf, theme| {
            prompt(
                0,
                "Source password (stored immediately, never displayed)",
                "",
                "[hidden]",
                true,
                0,
                area,
                buf,
                theme,
            )
        });
    }
}
