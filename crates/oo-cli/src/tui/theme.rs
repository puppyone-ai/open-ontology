//! One visual language for every `oo init` screen.
//!
//! Glyph brightness carries meaning on its own (monochrome still reads); colour
//! is a separate layer chosen from one small palette. Every screen uses the same
//! rounded frame, the same step rail in the title and the same key-hint footer.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Semantic tones. Screens never pick raw colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tone {
    /// Primary text.
    Ink,
    /// Secondary text.
    Dim,
    /// Structure: frames, rules, dot fields.
    Faint,
    /// The one accent: selection, focus, the brand mark.
    Accent,
    /// Pass, adopted.
    Ok,
    /// Needs a look, demoted, pending.
    Warn,
    /// Failed, rejected.
    Bad,
    /// Lines and links in the graph.
    Info,
    /// Deep layer of the warm axis: shadows, finished rail, low heat.
    Ember,
    /// Highlight layer of the warm axis: glint core, the one cell that just
    /// changed, peak heat. Never used for steady text.
    Hot,
}

impl Tone {
    /// One letter per tone for the colour-layer snapshots.
    #[cfg(test)]
    pub fn code(self) -> char {
        match self {
            Tone::Ink => 'i',
            Tone::Dim => 'd',
            Tone::Faint => 'f',
            Tone::Accent => 'A',
            Tone::Ok => 'G',
            Tone::Warn => 'Y',
            Tone::Bad => 'R',
            Tone::Info => 'B',
            Tone::Ember => 'E',
            Tone::Hot => 'H',
        }
    }
}

/// Rendering mode. `color == false` honours `NO_COLOR`; `motion == false`
/// freezes every effect on its resolved frame; `zh` picks Chinese copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub color: bool,
    pub motion: bool,
    pub zh: bool,
}

/// Interface language from the environment: `OO_LANG` first, then `LC_ALL`,
/// then `LANG`; Chinese when the first one set starts with `zh`.
pub fn lang_zh() -> bool {
    ["OO_LANG", "LC_ALL", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .is_some_and(|v| v.to_ascii_lowercase().starts_with("zh"))
}

impl Theme {
    #[cfg(test)]
    pub const MONO: Theme = Theme {
        color: false,
        motion: false,
        zh: false,
    };
    #[cfg(test)]
    pub const COLOR: Theme = Theme {
        color: true,
        motion: false,
        zh: false,
    };

    /// Terminal defaults: colour unless `NO_COLOR`; motion unless disabled or
    /// reduced; language from the locale. Callers already know whether stdout
    /// is a TTY.
    pub fn from_env(no_anim_flag: bool) -> Theme {
        let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
        let color = !set("NO_COLOR") && std::env::var("TERM").map_or(true, |t| t != "dumb");
        let motion = color
            && !no_anim_flag
            && !set("OO_NO_ANIM")
            && !set("OO_REDUCED_MOTION")
            && !set("REDUCE_MOTION");
        Theme {
            color,
            motion,
            zh: lang_zh(),
        }
    }

    /// The same theme with Chinese copy (tests and samples).
    #[cfg(test)]
    pub const fn zh(self) -> Theme {
        Theme { zh: true, ..self }
    }

    /// Pick the copy for the interface language.
    pub fn t<'a>(&self, en: &'a str, zh: &'a str) -> &'a str {
        if self.zh { zh } else { en }
    }

    /// Animation frame to draw: the live frame with motion, else the resolved
    /// still frame (`usize::MAX`).
    pub fn frame(&self, live: usize) -> usize {
        if self.motion { live } else { usize::MAX }
    }

    pub fn style(&self, tone: Tone) -> Style {
        if !self.color {
            return match tone {
                Tone::Accent | Tone::Bad | Tone::Hot => Style::new().add_modifier(Modifier::BOLD),
                Tone::Faint | Tone::Dim => Style::new().add_modifier(Modifier::DIM),
                _ => Style::new(),
            };
        }
        let fg = match tone {
            Tone::Ink => Color::Indexed(254),
            Tone::Dim => Color::Indexed(247),
            Tone::Faint => Color::Indexed(240),
            Tone::Accent => Color::Indexed(215),
            Tone::Ok => Color::Indexed(114),
            Tone::Warn => Color::Indexed(221),
            Tone::Bad => Color::Indexed(203),
            Tone::Info => Color::Indexed(110),
            Tone::Ember => Color::Indexed(173),
            Tone::Hot => Color::Indexed(223),
        };
        let style = Style::new().fg(fg);
        if matches!(tone, Tone::Accent | Tone::Hot) {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    }

    pub fn span<'a>(&self, text: impl Into<std::borrow::Cow<'a, str>>, tone: Tone) -> Span<'a> {
        Span::styled(text, self.style(tone))
    }

    /// Selection is reverse video in both modes so it never depends on colour.
    pub fn selected(&self) -> Style {
        self.style(Tone::Accent).add_modifier(Modifier::REVERSED)
    }

    /// Reverse lookup for snapshot colour layers.
    #[cfg(test)]
    pub fn tone_of(&self, style: Style) -> Option<Tone> {
        [
            Tone::Accent,
            Tone::Hot,
            Tone::Ember,
            Tone::Ok,
            Tone::Warn,
            Tone::Bad,
            Tone::Info,
            Tone::Ink,
            Tone::Dim,
            Tone::Faint,
        ]
        .into_iter()
        .find(|tone| {
            let s = self.style(*tone);
            s.fg.is_some() && s.fg == style.fg
        })
    }
}

/// Glyph vocabulary shared by all screens.
pub mod glyph {
    pub const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    /// Eighth blocks for smooth bars.
    pub const BAR: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];
    pub const STEP_DONE: &str = "●";
    pub const STEP_NOW: &str = "◉";
    pub const STEP_TODO: &str = "○";
    /// Sparkline levels.
    pub const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
}

/// Display width in terminal columns (CJK counts two).
pub fn width(text: &str) -> usize {
    Span::raw(text).width()
}

pub fn char_width(c: char) -> usize {
    let mut buf = [0u8; 4];
    Span::raw(&*c.encode_utf8(&mut buf)).width()
}

/// Truncate to at most `max` columns, ending with `…` when cut.
pub fn fit(text: &str, max: usize) -> String {
    if width(text) <= max {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = char_width(c);
        if used + w + 1 > max {
            break;
        }
        out.push(c);
        used += w;
    }
    if max > 0 {
        out.push('…');
    }
    out
}

/// Left-align `text` in exactly `cols` columns.
pub fn pad(text: &str, cols: usize) -> String {
    let text = fit(text, cols);
    let w = width(&text);
    format!("{text}{}", " ".repeat(cols.saturating_sub(w)))
}

pub const STEPS: [(&str, &str); 5] = [
    ("connect", "连接"),
    ("tables", "选表"),
    ("build", "构建"),
    ("confirm", "确认"),
    ("deliver", "交付"),
];

/// A key hint: key, English label, Chinese label.
pub type Hint = (&'static str, &'static str, &'static str);

/// The rounded frame every screen sits in: brand mark and step rail on the top
/// border, key hints on the bottom border. Returns the inner area.
pub fn frame(buf: &mut Buffer, area: Rect, theme: &Theme, step: usize, hints: &[Hint]) -> Rect {
    if area.width < 20 || area.height < 6 {
        return area;
    }
    let edge = theme.style(Tone::Faint);
    let (x0, y0) = (area.x, area.y);
    let (x1, y1) = (area.right() - 1, area.bottom() - 1);
    for x in x0..=x1 {
        buf[(x, y0)].set_symbol("─").set_style(edge);
        buf[(x, y1)].set_symbol("─").set_style(edge);
    }
    for y in y0..=y1 {
        buf[(x0, y)].set_symbol("│").set_style(edge);
        buf[(x1, y)].set_symbol("│").set_style(edge);
    }
    buf[(x0, y0)].set_symbol("╭").set_style(edge);
    buf[(x1, y0)].set_symbol("╮").set_style(edge);
    buf[(x0, y1)].set_symbol("╰").set_style(edge);
    buf[(x1, y1)].set_symbol("╯").set_style(edge);

    // Top: ─ ◆ open ontology ──── ● connect ─ ◉ tables ─ ○ build … ─
    let mut title = vec![
        theme.span(" ", Tone::Faint),
        theme.span("◆", Tone::Accent),
        theme.span(" open ontology ", Tone::Ink),
    ];
    // The step rail is a fuse: the stretch already walked is a heavy warm
    // line, the rest a faint thin one.
    let mut rail = Vec::new();
    let compact = area.width < 100;
    for (index, (en, zh)) in STEPS.iter().enumerate() {
        let name = theme.t(en, zh);
        let (mark, tone) = match index.cmp(&step) {
            std::cmp::Ordering::Less => (glyph::STEP_DONE, Tone::Ember),
            std::cmp::Ordering::Equal => (glyph::STEP_NOW, Tone::Accent),
            std::cmp::Ordering::Greater => (glyph::STEP_TODO, Tone::Faint),
        };
        if index > 0 {
            let walked = index <= step;
            let seg = match (compact, walked) {
                (true, true) => "━",
                (true, false) => "─",
                (false, true) => "━━",
                (false, false) => "──",
            };
            rail.push(theme.span(seg, if walked { Tone::Ember } else { Tone::Faint }));
        }
        rail.push(theme.span(mark, tone));
        if !compact || index == step {
            let name_tone = if index < step { Tone::Dim } else { tone };
            rail.push(theme.span(format!(" {name}"), name_tone));
        }
    }
    rail.push(theme.span(" ", Tone::Faint));
    let title_w: usize = title.iter().map(Span::width).sum();
    let rail_w: usize = rail.iter().map(Span::width).sum();
    let inner_w = usize::from(area.width).saturating_sub(4);
    if title_w + rail_w + 2 <= inner_w {
        buf.set_line(
            x0 + 1,
            y0,
            &Line::from(std::mem::take(&mut title)),
            area.width - 2,
        );
        let rx = x1 - 1 - rail_w as u16;
        buf.set_line(rx, y0, &Line::from(rail), rail_w as u16);
    } else {
        buf.set_line(x0 + 1, y0, &Line::from(title), area.width - 2);
    }

    // Bottom: key hints, dropped from the right when the frame is narrow.
    let mut spans = vec![theme.span(" ", Tone::Faint)];
    let mut used = 1;
    for (key, en, zh) in hints {
        let label = theme.t(en, zh);
        let need = width(key) + width(label) + 3;
        if used + need + 2 > inner_w {
            break;
        }
        spans.push(theme.span(*key, Tone::Accent));
        spans.push(theme.span(format!(" {label}"), Tone::Dim));
        spans.push(theme.span("  ", Tone::Faint));
        used += need;
    }
    buf.set_line(x0 + 1, y1, &Line::from(spans), area.width - 2);
    Rect::new(
        x0 + 2,
        y0 + 1,
        area.width.saturating_sub(4),
        area.height.saturating_sub(2),
    )
}

/// A heading with a dotted fade: `tables ·········· • ·`.
pub fn heading(buf: &mut Buffer, x: u16, y: u16, w: u16, theme: &Theme, text: &str, note: &str) {
    let mut spans = vec![theme.span(text.to_owned(), Tone::Ink)];
    let used = width(text) + 1;
    let note_w = if note.is_empty() { 0 } else { width(note) + 2 };
    let avail = usize::from(w).saturating_sub(used + note_w);
    if avail > 2 {
        spans.push(theme.span(" ", Tone::Faint));
        spans.push(theme.span(super::fx::fade_rule(avail), Tone::Faint));
    }
    if !note.is_empty() {
        spans.push(theme.span(format!("  {note}"), Tone::Dim));
    }
    buf.set_line(x, y, &Line::from(spans), w);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_count_cjk_as_two_and_fit_never_overflows() {
        assert_eq!(width("订单"), 4);
        assert_eq!(width("order"), 5);
        assert_eq!(fit("运输订单明细表", 9), "运输订单…");
        assert!(width(&fit("运输订单明细表", 9)) <= 9);
        assert_eq!(pad("订单", 6), "订单  ");
    }

    #[test]
    fn mono_styles_carry_no_colour() {
        for tone in [Tone::Ink, Tone::Accent, Tone::Bad, Tone::Ok] {
            assert_eq!(Theme::MONO.style(tone).fg, None);
        }
        assert_eq!(
            Theme::COLOR.tone_of(Theme::COLOR.style(Tone::Ok)),
            Some(Tone::Ok)
        );
    }
}
