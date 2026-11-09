//! Screen 5: deliver. The token goes into a named CLI profile; the share
//! prompt is shown ready to copy; `m` prints MCP configuration that names the
//! profile only; a readiness check gives the final light.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::{Action, Check, Status, checklist, put};
use crate::tui::backend::McpClient;
use crate::tui::fx;
use crate::tui::theme::{self, Hint, Theme, Tone, fit, heading, width};

#[derive(Clone, Debug)]
pub struct Deliver {
    pub profile: String,
    pub profile_note: Result<String, String>,
    pub share_prompt: String,
    pub copied: bool,
    pub mcp_menu: bool,
    pub mcp_choice: usize,
    pub mcp: Option<(McpClient, String)>,
    pub checks: Vec<Check>,
    pub frame: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    None,
    Copy,
    Mcp(McpClient),
    Recheck,
}

impl Deliver {
    pub fn ready(&self) -> bool {
        !self.checks.is_empty() && self.checks.iter().all(|c| c.status == Status::Pass)
    }
}

/// Returns the screen action and a request for the runner (clipboard, MCP
/// generation, re-check); the screen itself performs no I/O.
pub fn key(state: &mut Deliver, key: KeyEvent) -> (Action, Request) {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return (Action::Quit, Request::None);
    }
    if state.mcp_menu {
        match key.code {
            KeyCode::Esc => state.mcp_menu = false,
            KeyCode::Up => state.mcp_choice = state.mcp_choice.saturating_sub(1),
            KeyCode::Down => {
                state.mcp_choice = (state.mcp_choice + 1).min(McpClient::ALL.len() - 1)
            }
            KeyCode::Enter => {
                state.mcp_menu = false;
                return (Action::None, Request::Mcp(McpClient::ALL[state.mcp_choice]));
            }
            _ => (),
        }
        return (Action::None, Request::None);
    }
    match key.code {
        KeyCode::Char('c') => (Action::None, Request::Copy),
        KeyCode::Char('m') => {
            state.mcp_menu = true;
            (Action::None, Request::None)
        }
        KeyCode::Char('r') => (Action::None, Request::Recheck),
        KeyCode::Char('p') => {
            state.mcp = None;
            state.copied = false;
            (Action::None, Request::None)
        }
        KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => (Action::Next, Request::None),
        _ => (Action::None, Request::None),
    }
}

pub const HINTS: [Hint; 5] = [
    ("c", "copy", "复制"),
    ("m", "MCP config", "MCP 配置"),
    ("p", "prompt", "提示词"),
    ("r", "re-check", "重新检查"),
    ("⏎", "done", "完成"),
];

/// Frames until the ready moment: one readiness row a frame.
pub fn ready_frame(state: &Deliver) -> usize {
    state.checks.len()
}

/// Word-wrap by display width.
fn wrap(text: &str, cols: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            if !line.is_empty() && width(&line) + 1 + width(word) > cols {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    lines
}

pub fn render(state: &Deliver, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let inner = theme::frame(buf, area, theme, 4, &HINTS);
    if inner.height < 10 {
        return;
    }
    heading(
        buf,
        inner.x,
        inner.y,
        inner.width,
        theme,
        theme.t("deliver", "交付"),
        theme.t(
            "CLI profile + share prompt · MCP optional",
            "CLI profile + 分享提示词 · MCP 可选",
        ),
    );
    let frame = theme.frame(state.frame);
    let mut y = inner.y + 2;
    let (mark, tone, note) = match &state.profile_note {
        Ok(note) => ("●", Tone::Ok, note.clone()),
        Err(note) => ("○", Tone::Warn, note.clone()),
    };
    put(
        buf,
        inner.x,
        y,
        inner.width,
        vec![
            theme.span(theme.t("profile  ", "profile  "), Tone::Dim),
            theme.span(state.profile.clone(), Tone::Accent),
            theme.span(format!("   {mark} "), tone),
            theme.span(
                fit(
                    &note,
                    usize::from(inner.width).saturating_sub(width(&state.profile) + 14),
                ),
                Tone::Dim,
            ),
        ],
    );
    y += 2;
    // The card shows the share prompt, or the MCP snippet once chosen.
    let card_w = inner.width.min(100);
    let text_w = usize::from(card_w).saturating_sub(4);
    let (title, note, body) = match &state.mcp {
        Some((client, text)) => (
            format!("MCP · {}", client.name()),
            theme.t(
                "profile name only, no credentials · p prompt",
                "只含 profile 名，不含凭据 · p 看提示词",
            ),
            text.clone(),
        ),
        None => (
            theme.t("share prompt", "分享提示词").to_owned(),
            "",
            state.share_prompt.clone(),
        ),
    };
    let lines = wrap(&body, text_w);
    let max_lines = usize::from(inner.height.saturating_sub(15)).clamp(3, 10);
    let copied = if state.copied {
        theme.t("  ✓ copied", "  ✓ 已复制")
    } else {
        theme.t("  c copy", "  c 复制")
    };
    put(
        buf,
        inner.x,
        y,
        card_w,
        vec![
            theme.span(title, Tone::Ink),
            theme.span(copied, if state.copied { Tone::Ok } else { Tone::Faint }),
            theme.span(format!("  {note}"), Tone::Faint),
        ],
    );
    y += 1;
    let edge = |l: &str, r: &str| {
        format!(
            "{l}{}{r}",
            "┄".repeat(usize::from(card_w).saturating_sub(2))
        )
    };
    put(
        buf,
        inner.x,
        y,
        card_w,
        vec![theme.span(edge("╭", "╮"), Tone::Faint)],
    );
    y += 1;
    let tone = if state.mcp.is_some() {
        Tone::Info
    } else {
        Tone::Ink
    };
    for line in lines.iter().take(max_lines) {
        put(
            buf,
            inner.x,
            y,
            card_w,
            vec![
                theme.span("┆ ", Tone::Faint),
                theme.span(theme::pad(line, text_w), tone),
                theme.span(" ┆", Tone::Faint),
            ],
        );
        y += 1;
    }
    if lines.len() > max_lines {
        put(
            buf,
            inner.x,
            y,
            card_w,
            vec![
                theme.span("┆ ", Tone::Faint),
                theme.span(
                    theme::pad(
                        &if theme.zh {
                            format!("… 还有 {} 行（复制时完整）", lines.len() - max_lines)
                        } else {
                            format!("… {} more lines (copied in full)", lines.len() - max_lines)
                        },
                        text_w,
                    ),
                    Tone::Faint,
                ),
                theme.span(" ┆", Tone::Faint),
            ],
        );
        y += 1;
    }
    put(
        buf,
        inner.x,
        y,
        card_w,
        vec![theme.span(edge("╰", "╯"), Tone::Faint)],
    );
    y += 2;
    put(
        buf,
        inner.x,
        y,
        inner.width,
        vec![theme.span(theme.t("readiness", "就绪检查"), Tone::Ink)],
    );
    let shown = if frame == usize::MAX {
        usize::MAX
    } else {
        frame + 1
    };
    checklist(
        buf,
        Rect::new(
            inner.x,
            y + 1,
            inner.width,
            inner.bottom().saturating_sub(y + 3),
        ),
        theme,
        &state.checks,
        state.frame,
        shown,
    );
    // Final light. Ready gets a ceremony: once every check has ticked in, a
    // banner with a glint across it and a one-shot burst over the ✓.
    let foot = inner.bottom() - 1;
    let settled = frame == usize::MAX || frame >= ready_frame(state);
    if state.ready() && settled {
        let local = if frame == usize::MAX {
            usize::MAX
        } else {
            frame - ready_frame(state)
        };
        banner(buf, Rect::new(inner.x, foot, inner.width, 1), theme, local);
        if local != usize::MAX {
            // Centred over the banner's words, in the blank rows above it.
            let words = theme::width(theme.t(
                "✓ ready · hand the prompt to your agent",
                "✓ 就绪 · 把提示词交给你的 Agent",
            ));
            let ox = i32::from(inner.x) + (words / 2) as i32;
            for (dx, dy, c, tone) in fx::burst(local) {
                let x = ox + dx;
                let y = i32::from(foot) - 2 + dy;
                if x < i32::from(inner.x)
                    || x >= i32::from(inner.right())
                    || y < i32::from(inner.y)
                    || y >= i32::from(foot)
                {
                    continue;
                }
                let cell = &mut buf[(x as u16, y as u16)];
                if cell.symbol() == " " {
                    let mut tmp = [0u8; 4];
                    cell.set_symbol(c.encode_utf8(&mut tmp))
                        .set_style(theme.style(tone));
                }
            }
        }
    } else {
        let (light, tone, text) = if !settled {
            ("○", Tone::Faint, theme.t("checking…", "检查中…"))
        } else if state.checks.iter().any(|c| c.status == Status::Fail) {
            (
                "●",
                Tone::Bad,
                theme.t("not ready: see the failed check", "未就绪：看失败的那一项"),
            )
        } else {
            (
                "○",
                Tone::Warn,
                theme.t("readiness not confirmed yet", "就绪状态还没确认"),
            )
        };
        put(
            buf,
            inner.x,
            foot,
            inner.width,
            vec![
                theme.span(format!("{light} "), tone),
                theme.span(text, Tone::Ink),
            ],
        );
    }
    if state.mcp_menu {
        let m = super::modal(
            buf,
            area,
            theme,
            theme.t("MCP configuration for", "生成 MCP 配置"),
            40,
            7,
        );
        for (k, client) in McpClient::ALL.iter().enumerate() {
            let on = k == state.mcp_choice;
            put(
                buf,
                m.x,
                m.y + k as u16,
                m.width,
                vec![
                    theme.span(if on { "▸ " } else { "  " }, Tone::Accent),
                    theme.span(client.name(), if on { Tone::Ink } else { Tone::Dim }),
                ],
            );
        }
        put(
            buf,
            m.x,
            m.y + 4,
            m.width,
            vec![theme.span(
                theme.t("⏎ show   esc back", "⏎ 显示   esc 返回"),
                Tone::Faint,
            )],
        );
    }
}

/// Frames the ready banner's glint takes to cross.
const GLINT_FRAMES: usize = 8;

/// `✓ ready · hand the prompt to your agent ━━━━━━` with a glint crossing it
/// in the first frames after the ready moment (after ascii.rest kit `glint`).
fn banner(buf: &mut Buffer, area: Rect, theme: &Theme, local: usize) {
    let text = theme.t(
        "ready · hand the prompt to your agent",
        "就绪 · 把提示词交给你的 Agent",
    );
    let mut cells: Vec<(String, Tone)> = vec![("✓".into(), Tone::Ok), (" ".into(), Tone::Faint)];
    for c in text.chars() {
        cells.push((c.to_string(), Tone::Ink));
    }
    cells.push((" ".into(), Tone::Faint));
    let used: usize = cells.iter().map(|(c, _)| width(c)).sum();
    for _ in used..usize::from(area.width) {
        cells.push(("━".into(), Tone::Ember));
    }
    if local < GLINT_FRAMES {
        let at = (local as f32 + 0.5) / GLINT_FRAMES as f32 * (cells.len() as f32 + 6.0) - 3.0;
        let mut x = 0f32;
        for (c, tone) in &mut cells {
            let d = (x - at).abs();
            if d < 2.4 && c.as_str() != " " {
                *tone = Tone::Hot;
            }
            x += width(c) as f32;
        }
    }
    let spans = cells.into_iter().map(|(c, t)| theme.span(c, t)).collect();
    put(buf, area.x, area.y, area.width, spans);
}

#[cfg(test)]
mod tests {
    use super::super::snapshot;
    use super::*;
    use crate::tui::backend::{Delivery, Pending, mcp_config};

    pub(crate) fn sample() -> Deliver {
        Deliver {
            profile: "logistics".into(),
            profile_note: Ok("token saved · switch with oo use logistics".into()),
            share_prompt: Pending.share_prompt("logistics"),
            copied: true,
            mcp_menu: false,
            mcp_choice: 0,
            mcp: Some((McpClient::Codex, mcp_config(McpClient::Codex, "logistics"))),
            checks: vec![
                Check::new("pack published", Status::Pass, "generation 3 · 13 entities"),
                Check::new(
                    "gateway reachable",
                    Status::Pass,
                    "https://127.0.0.1:8443 · 4 ms",
                ),
                Check::new("visible tables", Status::Pass, "11 tables for role analyst"),
                Check::new("freshness", Status::Pass, "published 2 min ago"),
            ],
            frame: 0,
        }
    }

    #[test]
    fn keys_request_io_from_the_runner() {
        let mut s = sample();
        assert_eq!(
            key(&mut s, KeyEvent::from(KeyCode::Char('c'))).1,
            Request::Copy
        );
        key(&mut s, KeyEvent::from(KeyCode::Char('m')));
        assert!(s.mcp_menu);
        key(&mut s, KeyEvent::from(KeyCode::Down));
        assert_eq!(
            key(&mut s, KeyEvent::from(KeyCode::Enter)).1,
            Request::Mcp(McpClient::Cursor)
        );
        assert!(s.ready());
    }

    #[test]
    fn snapshots() {
        let state = sample();
        snapshot::all_sizes("5-deliver", |area, buf, theme| {
            render(&state, area, buf, theme)
        });
        let mut pending = sample();
        pending.profile_note = Err("profile store not available in this build (V3-B-01)".into());
        pending.copied = false;
        pending.mcp = None;
        pending.mcp_menu = true;
        pending.checks = Pending.check("logistics");
        snapshot::check(
            "5-deliver-pending",
            80,
            24,
            Theme::MONO,
            |area, buf, theme| render(&pending, area, buf, theme),
        );
    }

    #[test]
    fn ready_ceremony_frames() {
        let mut state = sample();
        state.mcp = None;
        state.copied = false;
        let frames: Vec<(usize, &str)> = (0..15)
            .step_by(2)
            .map(|f| (f, "checks tick in, then glint and burst"))
            .chain([(usize::MAX, "still")])
            .collect();
        snapshot::frames(
            "5-deliver",
            80,
            24,
            false,
            &frames,
            |f, area, buf, theme| {
                let mut s = state.clone();
                s.frame = f;
                render(&s, area, buf, theme);
            },
        );
    }
}
