//! The only code that touches the terminal: raw mode, the alternate screen,
//! key polling and the per-screen loops. Callers check for a TTY first; none
//! of this runs for non-TTY or `--json` paths.
use std::io::{self, Write};
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEvent, KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use serde_json::Value;

use super::adjudication::{Adjudication, Overrides};
use super::backend::{Delivery, osc52};
use super::fx::{SPLASH_FRAME_MS, SPLASH_FRAMES};
use super::graph::REVEAL_FRAMES;
use super::scorer::TableMeta;
use super::screens::{self, Action};
use super::theme::Theme;

pub type Result<T> = std::result::Result<T, &'static str>;
const TICK: Duration = Duration::from_millis(100);
/// Tick while a one-shot animation plays.
const FAST: Duration = Duration::from_millis(70);

/// A key press or a bracketed paste.
pub enum Input {
    Key(KeyEvent),
    Paste(String),
}

pub struct Term {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    pub theme: Theme,
}

impl Term {
    pub fn open(theme: Theme) -> Result<Self> {
        enable_raw_mode().map_err(|_| "TUI_TERMINAL")?;
        if execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste).is_err() {
            let _ = disable_raw_mode();
            return Err("TUI_TERMINAL");
        }
        match Terminal::new(CrosstermBackend::new(io::stdout())) {
            Ok(terminal) => Ok(Term { terminal, theme }),
            Err(_) => {
                let _ = execute!(io::stdout(), LeaveAlternateScreen);
                let _ = disable_raw_mode();
                Err("TUI_TERMINAL")
            }
        }
    }

    pub fn draw(&mut self, mut paint: impl FnMut(Rect, &mut Buffer, &Theme)) -> Result<()> {
        let theme = self.theme;
        self.terminal
            .draw(|frame| {
                let area = frame.area();
                paint(area, frame.buffer_mut(), &theme);
            })
            .map(|_| ())
            .map_err(|_| "TUI_RENDER")
    }

    pub fn width(&self) -> u16 {
        self.terminal.size().map_or(80, |s| s.width)
    }

    /// Next key press within `wait`, if any.
    pub fn key(&mut self, wait: Duration) -> Result<Option<KeyEvent>> {
        if !event::poll(wait).map_err(|_| "TUI_INPUT")? {
            return Ok(None);
        }
        match event::read().map_err(|_| "TUI_INPUT")? {
            Event::Key(key) if key.kind == KeyEventKind::Press => Ok(Some(key)),
            _ => Ok(None),
        }
    }

    /// Next key press or bracketed paste within `wait`.
    pub fn input(&mut self, wait: Duration) -> Result<Option<Input>> {
        if !event::poll(wait).map_err(|_| "TUI_INPUT")? {
            return Ok(None);
        }
        match event::read().map_err(|_| "TUI_INPUT")? {
            Event::Key(key) if key.kind == KeyEventKind::Press => Ok(Some(Input::Key(key))),
            Event::Paste(text) => Ok(Some(Input::Paste(text))),
            _ => Ok(None),
        }
    }

    /// Copy through the terminal clipboard escape (OSC 52).
    pub fn copy(&mut self, text: &str) -> bool {
        let mut out = io::stdout();
        out.write_all(osc52(text).as_bytes())
            .and_then(|_| out.flush())
            .is_ok()
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
    }
}

/// ≤ 1.5 s, skippable with any key. Without motion: the resolved still frame,
/// held briefly (also skippable).
pub fn splash(term: &mut Term) -> Result<()> {
    if !term.theme.motion {
        term.draw(|area, buf, theme| screens::splash(usize::MAX, area, buf, theme))?;
        term.key(Duration::from_millis(600))?;
        return Ok(());
    }
    for frame in 0..SPLASH_FRAMES {
        term.draw(|area, buf, theme| screens::splash(frame, area, buf, theme))?;
        if term.key(Duration::from_millis(SPLASH_FRAME_MS))?.is_some() {
            return Ok(());
        }
    }
    term.key(Duration::from_millis(SPLASH_FRAME_MS * 2))?;
    Ok(())
}

/// The I/O screen 1 needs. `Network` covers everything that needs no
/// credentials; the wizard supplies sign-in.
pub trait Live: Sync {
    fn resolve(
        &self,
        host: &str,
        port: u16,
    ) -> std::result::Result<Vec<std::net::IpAddr>, &'static str> {
        super::netprobe::resolve(host, port)
    }
    fn port(&self, host: &str, port: u16) -> std::result::Result<Duration, &'static str> {
        super::netprobe::port_open(host, port)
    }
    fn tls(
        &self,
        engine: super::source::Engine,
        host: &str,
        port: u16,
    ) -> std::result::Result<super::netprobe::TlsReport, &'static str> {
        super::netprobe::tls_chain(engine, host, port)
    }
    fn verify(&self, request: &screens::connect::Request) -> screens::connect::Verified;
}

fn run_job(live: &dyn Live, job: screens::connect::Job) -> screens::connect::Outcome {
    use screens::connect::{Job, Outcome};
    match job {
        Job::Resolve(h) => Outcome::Resolved(h.clone(), live.resolve(&h, 0)),
        Job::Port(h, p) => Outcome::Port(h.clone(), p, live.port(&h, p)),
        Job::Tls(e, h, p) => Outcome::Tls(h.clone(), p, live.tls(e, &h, p)),
        Job::Verify(r) => Outcome::Verified(live.verify(&r)),
    }
}

/// Screen 1. Live checks run on background threads; ⏎ signs in through
/// `live.verify`. Returns the final state (demo chosen, or a source that
/// signed in), or `None` on quit.
pub fn connect(
    term: &mut Term,
    mut state: screens::connect::Connect,
    live: &dyn Live,
) -> Result<Option<screens::connect::Connect>> {
    use std::collections::HashSet;
    let (tx, rx) = std::sync::mpsc::channel::<screens::connect::Outcome>();
    std::thread::scope(|scope| {
        let mut started: HashSet<String> = HashSet::new();
        loop {
            while let Ok(outcome) = rx.try_recv() {
                started.remove(&outcome.key());
                state.apply(outcome);
            }
            if state.pass_mode == screens::connect::PassMode::Env {
                state.env_set =
                    std::env::var_os(state.pass_env.trim()).is_some_and(|v| !v.is_empty());
            }
            let mut jobs = state.wants();
            jobs.extend(state.take_verify());
            for job in jobs {
                if !started.insert(job.key()) {
                    continue;
                }
                let tx = tx.clone();
                scope.spawn(move || {
                    let _ = tx.send(run_job(live, job));
                });
            }
            term.draw(|area, buf, theme| screens::connect::render(&state, area, buf, theme))?;
            let Some(event) = term.input(TICK)? else {
                state.frame += 1;
                continue;
            };
            state.frame += 1;
            let action = match event {
                Input::Paste(text) => {
                    screens::connect::paste(&mut state, &text);
                    Action::None
                }
                Input::Key(key) => screens::connect::key(&mut state, key),
            };
            match action {
                Action::Quit => return Ok(None),
                Action::Next => return Ok(Some(state)),
                Action::None => (),
            }
        }
    })
}

/// Screen 2. Returns the explicit table list, or `None` on quit.
pub fn select(term: &mut Term, tables: &[TableMeta]) -> Result<Option<Vec<String>>> {
    let mut state = screens::select::Select::new(tables);
    loop {
        term.draw(|area, buf, theme| screens::select::render(&state, area, buf, theme))?;
        let sweeping = term.theme.motion && state.frame < screens::select::SWEEP_FRAMES;
        let key = term.key(if sweeping { FAST } else { TICK })?;
        state.frame = state.frame.saturating_add(1);
        if let Some(key) = key {
            match screens::select::key(&mut state, key) {
                Action::Quit => return Ok(None),
                Action::Next => return Ok(Some(state.selection())),
                Action::None => (),
            }
        }
    }
}

/// Screen 3. `poll` returns the latest chain state (or `None` when unchanged)
/// and whether the build has finished. Returns false if the user stopped
/// watching.
pub fn build(term: &mut Term, poll: &mut dyn FnMut() -> (Option<Value>, bool)) -> Result<bool> {
    let mut state = screens::build::Build::new();
    let started = Instant::now();
    let mut sampled = Instant::now();
    loop {
        let (latest, finished) = poll();
        if let Some(latest) = latest {
            let (frame, history) = (state.frame, std::mem::take(&mut state.history));
            state = screens::build::from_chain(&latest);
            state.frame = frame;
            state.history = history;
        }
        state.frame += 1;
        // One throughput sample a second, the last minute kept.
        if sampled.elapsed() >= Duration::from_secs(1) {
            sampled = Instant::now();
            state.history.push(state.completed());
            if state.history.len() > 61 {
                state.history.remove(0);
            }
        }
        if finished {
            let secs = started.elapsed().as_secs();
            state.note = if !state.done() {
                term.theme
                    .t(
                        "pipeline not complete · ⏎ continue",
                        "构建链未完成 · ⏎ 继续",
                    )
                    .into()
            } else if term.theme.zh {
                format!("用时 {secs} 秒 · ⏎ 继续")
            } else {
                format!("finished in {secs}s · ⏎ continue")
            };
        }
        term.draw(|area, buf, theme| screens::build::render(&state, area, buf, theme))?;
        if let Some(key) = term.key(TICK)? {
            match key.code {
                crossterm::event::KeyCode::Char('q') | crossterm::event::KeyCode::Esc => {
                    return Ok(false);
                }
                crossterm::event::KeyCode::Enter if finished => return Ok(true),
                _ => (),
            }
        }
    }
}

/// Screen 4. Returns `(publish-confirm document, overrides)` or `None`.
pub fn confirm(
    term: &mut Term,
    adj: Adjudication,
    now: &str,
) -> Result<Option<(Value, Overrides)>> {
    let mut state = screens::confirm::Confirm::new(adj, now);
    loop {
        state.width = term.width();
        term.draw(|area, buf, theme| screens::confirm::render(&state, area, buf, theme))?;
        let revealing = term.theme.motion && state.frame < REVEAL_FRAMES;
        let key = term.key(if revealing { FAST } else { TICK })?;
        state.frame = state.frame.saturating_add(1);
        if revealing && key.is_some() {
            // The first key only skips the reveal.
            state.frame = REVEAL_FRAMES;
            continue;
        }
        if let Some(key) = key {
            match screens::confirm::key(&mut state, key) {
                Action::Quit => return Ok(None),
                Action::Next => {
                    let doc = state.confirmed.take().ok_or("TUI_CONFIRM")?;
                    return Ok(Some((doc, state.overrides)));
                }
                Action::None => (),
            }
        }
    }
}

/// Screen 5.
pub fn deliver(term: &mut Term, delivery: &dyn Delivery, profile: &str) -> Result<()> {
    use screens::deliver::{Deliver, Request};
    let mut state = Deliver {
        profile: profile.into(),
        profile_note: delivery.save_profile(profile),
        share_prompt: delivery.share_prompt(profile),
        copied: false,
        mcp_menu: false,
        mcp_choice: 0,
        mcp: None,
        checks: delivery.check(profile),
        frame: 0,
    };
    let ceremony = screens::deliver::ready_frame(&state) + super::fx::BURST_FRAMES;
    loop {
        term.draw(|area, buf, theme| screens::deliver::render(&state, area, buf, theme))?;
        let busy = term.theme.motion && state.frame < ceremony;
        let Some(key) = term.key(if busy { FAST } else { TICK })? else {
            state.frame += 1;
            continue;
        };
        if busy {
            // The first key during the ceremony only finishes it.
            state.frame = ceremony;
            continue;
        }
        let (action, request) = screens::deliver::key(&mut state, key);
        match request {
            Request::Copy => {
                let text = state
                    .mcp
                    .as_ref()
                    .map_or(state.share_prompt.clone(), |m| m.1.clone());
                state.copied = term.copy(&text);
            }
            Request::Mcp(client) => {
                state.mcp = Some((client, delivery.mcp_config(client, profile)));
                state.copied = false;
            }
            Request::Recheck => state.checks = delivery.check(profile),
            Request::None => (),
        }
        if action != Action::None {
            return Ok(());
        }
    }
}

/// A themed single-line prompt (used by the remaining legacy questions).
pub fn prompt(
    term: &mut Term,
    step: usize,
    label: &str,
    default: &str,
    secret: bool,
) -> Result<String> {
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut value = String::new();
    let mut scroll = 0u16;
    loop {
        // Secret contents and lengths never enter a render buffer.
        let shown = if secret {
            "[hidden]".to_owned()
        } else {
            value.clone()
        };
        term.draw(|area, buf, theme| {
            screens::prompt(
                step, label, default, &shown, secret, scroll, area, buf, theme,
            );
        })?;
        let Some(key) = term.key(TICK)? else { continue };
        if key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            return Err("TUI_CANCELLED");
        }
        match key.code {
            KeyCode::Up => scroll = scroll.saturating_sub(1),
            KeyCode::Down => scroll = scroll.saturating_add(1),
            KeyCode::PageUp => scroll = scroll.saturating_sub(10),
            KeyCode::PageDown => scroll = scroll.saturating_add(10),
            KeyCode::Enter => {
                return Ok(if value.is_empty() {
                    default.into()
                } else {
                    value
                });
            }
            KeyCode::Backspace => {
                value.pop();
            }
            KeyCode::Char(c)
                if !c.is_control()
                    && value.len() + c.len_utf8() <= 4096
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                value.push(c);
            }
            _ => (),
        }
    }
}
