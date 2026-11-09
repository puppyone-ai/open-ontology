//! Screen 1: connect. Paste a connection string, fill in fields, or take the
//! demo. The password is masked with a fixed-width placeholder: neither its
//! value nor its length ever reaches a render buffer.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::{Action, Check, checklist, input, put};
use crate::tui::theme::{self, Hint, Theme, Tone, fit, heading};

pub const MODES: [(&str, &str); 3] = [
    ("paste a connection string", "粘贴连接串"),
    ("fill in fields", "逐项填写"),
    ("no database? use the demo", "没有数据库？用示例库"),
];
pub const FIELDS: [(&str, &str); 7] = [
    ("engine", "引擎"),
    ("host", "主机"),
    ("port", "端口"),
    ("database", "数据库"),
    ("user", "用户"),
    ("password", "密码"),
    ("CA file", "CA 文件"),
];
const MASK: &str = "••••••";

/// Parsed connection string. `password` is held only to be moved into the
/// secret store; `Debug` never prints it.
#[derive(Clone, PartialEq, Eq)]
pub struct Dsn {
    pub engine: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: Option<String>,
}

impl std::fmt::Debug for Dsn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Dsn({} {}:{} / {} as {}, password {})",
            self.engine,
            self.host,
            self.port,
            self.database,
            self.user,
            if self.password.is_some() {
                "set"
            } else {
                "unset"
            }
        )
    }
}

pub fn default_port(engine: &str) -> Option<u16> {
    Some(match engine {
        "postgres" | "kingbase" => 5432,
        "mysql" => 3306,
        "oracle" => 1521,
        "sqlserver" => 1433,
        "dm8" => 5236,
        "db2" => 50000,
        _ => return None,
    })
}

fn engine_of(scheme: &str) -> Option<&'static str> {
    Some(match scheme.to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" | "pg" => "postgres",
        "mysql" | "mariadb" => "mysql",
        "oracle" | "oracle:thin" => "oracle",
        "sqlserver" | "mssql" => "sqlserver",
        "kingbase" | "kingbase8" => "kingbase",
        "dm" | "dm8" | "dameng" => "dm8",
        "db2" => "db2",
        _ => return None,
    })
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(v) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `scheme://user:password@host:port/database?params`; `jdbc:` is accepted.
pub fn parse_dsn(raw: &str) -> Result<Dsn, &'static str> {
    let raw = raw.trim();
    let raw = raw.strip_prefix("jdbc:").unwrap_or(raw);
    let (scheme, rest) = raw.split_once("://").ok_or("DSN_SCHEME")?;
    let engine = engine_of(scheme).ok_or("DSN_ENGINE_UNKNOWN")?;
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let (auth, location) = match rest.rfind('@') {
        Some(at) => (Some(&rest[..at]), &rest[at + 1..]),
        None => (None, rest),
    };
    let (hostport, database) = location.split_once('/').unwrap_or((location, ""));
    let (host, port) = if let Some(stripped) = hostport.strip_prefix('[') {
        let (h, tail) = stripped.split_once(']').ok_or("DSN_HOST")?;
        (h, tail.strip_prefix(':'))
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (hostport, None),
        }
    };
    if host.is_empty() {
        return Err("DSN_HOST");
    }
    let port = match port {
        Some(p) => p
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .ok_or("DSN_PORT")?,
        None => default_port(engine).ok_or("DSN_PORT")?,
    };
    let (user, password) = match auth {
        Some(a) => match a.split_once(':') {
            Some((u, p)) => (decode(u), Some(decode(p))),
            None => (decode(a), None),
        },
        None => (String::new(), None),
    };
    Ok(Dsn {
        engine: engine.into(),
        host: host.into(),
        port,
        database: decode(database),
        user,
        password,
    })
}

/// Mask the password section of a (possibly half-typed) connection string.
pub fn mask(raw: &str) -> String {
    let Some(start) = raw.find("://").map(|i| i + 3) else {
        return raw.into();
    };
    let tail = &raw[start..];
    let at = tail.rfind('@');
    let auth_end = at.unwrap_or(tail.len());
    match tail[..auth_end].find(':') {
        Some(colon) => {
            let rest = at.map_or("", |a| &tail[a..]);
            format!("{}{}:{MASK}{rest}", &raw[..start], &tail[..colon])
        }
        None => raw.into(),
    }
}

#[derive(Clone, Debug, Default)]
pub struct Connect {
    pub mode: usize,
    pub dsn: String,
    pub fields: [String; 7],
    pub field: usize,
    pub checks: Vec<Check>,
    pub frame: usize,
    /// Frame at which the current checks arrived; rows reveal one a frame.
    pub checks_at: usize,
    pub error: Option<String>,
    /// Engine label once detected.
    pub detected: Option<String>,
}

impl Connect {
    pub fn new() -> Self {
        Connect::default()
    }

    /// The connection the user described, if complete enough to try.
    pub fn dsn(&self) -> Result<Dsn, &'static str> {
        match self.mode {
            0 => parse_dsn(&self.dsn),
            1 => {
                let f = &self.fields;
                let engine = engine_of(f[0].trim()).ok_or("DSN_ENGINE_UNKNOWN")?;
                if f[1].trim().is_empty() {
                    return Err("DSN_HOST");
                }
                let port = if f[2].trim().is_empty() {
                    default_port(engine).ok_or("DSN_PORT")?
                } else {
                    f[2].trim().parse().map_err(|_| "DSN_PORT")?
                };
                Ok(Dsn {
                    engine: engine.into(),
                    host: f[1].trim().into(),
                    port,
                    database: f[3].trim().into(),
                    user: f[4].trim().into(),
                    password: (!f[5].is_empty()).then(|| f[5].clone()),
                })
            }
            _ => Err("DSN_DEMO"),
        }
    }

    pub fn ca_path(&self) -> Option<&str> {
        let ca = self.fields[6].trim();
        (!ca.is_empty()).then_some(ca)
    }

    fn refresh(&mut self) {
        self.detected = self.dsn().ok().map(|d| d.engine);
        self.error = None;
    }
}

pub fn key(state: &mut Connect, key: KeyEvent) -> Action {
    if key.code == KeyCode::Esc
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        return Action::Quit;
    }
    match key.code {
        KeyCode::Up => state.mode = state.mode.saturating_sub(1),
        KeyCode::Down => state.mode = (state.mode + 1).min(MODES.len() - 1),
        KeyCode::Tab if state.mode == 1 => state.field = (state.field + 1) % FIELDS.len(),
        KeyCode::BackTab if state.mode == 1 => {
            state.field = (state.field + FIELDS.len() - 1) % FIELDS.len()
        }
        KeyCode::Enter => {
            if state.mode == 2 {
                return Action::Next;
            }
            match state.dsn() {
                Ok(_) => return Action::Next,
                Err(e) => state.error = Some(e.into()),
            }
        }
        KeyCode::Backspace => {
            match state.mode {
                0 => {
                    state.dsn.pop();
                }
                1 => {
                    state.fields[state.field].pop();
                }
                _ => (),
            }
            state.refresh();
        }
        KeyCode::Char(c)
            if !c.is_control()
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            let target = match state.mode {
                0 => &mut state.dsn,
                1 => &mut state.fields[state.field],
                _ => return Action::None,
            };
            if target.len() + c.len_utf8() <= 4096 {
                target.push(c);
            }
            state.refresh();
        }
        _ => (),
    }
    Action::None
}

pub const HINTS: [Hint; 4] = [
    ("↑↓", "choose", "选择"),
    ("tab", "next field", "下一项"),
    ("⏎", "connect", "连接"),
    ("esc", "quit", "退出"),
];

pub fn render(state: &Connect, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let inner = theme::frame(buf, area, theme, 0, &HINTS);
    if inner.height < 8 {
        return;
    }
    let wide = inner.width >= 100;
    heading(
        buf,
        inner.x,
        inner.y,
        inner.width,
        theme,
        theme.t("connect", "连接"),
        theme.t(
            "read-only · nothing is written to your database",
            "只读 · 不会写入你的数据库",
        ),
    );
    let left_w: u16 = if wide { 32 } else { inner.width };
    let mut y = inner.y + 2;
    for (index, (en, zh)) in MODES.iter().enumerate() {
        let on = index == state.mode;
        put(
            buf,
            inner.x,
            y,
            left_w,
            vec![
                theme.span(
                    if on { "◉ " } else { "○ " },
                    if on { Tone::Accent } else { Tone::Faint },
                ),
                theme.span(theme.t(en, zh), if on { Tone::Ink } else { Tone::Dim }),
            ],
        );
        y += 1;
    }
    // Right column (or below the options on narrow screens).
    let (rx, mut ry, rw) = if wide {
        (inner.x + left_w + 2, inner.y + 2, inner.width - left_w - 2)
    } else {
        (inner.x, y + 1, inner.width)
    };
    match state.mode {
        0 => {
            input(buf, rx, ry, rw, theme, &mask(&state.dsn), true);
            ry += 1;
            let hint = match (state.dsn.is_empty(), state.dsn()) {
                (true, _) => vec![theme.span(
                    format!(
                        "{} postgres://reader@db.internal:5432/logistics",
                        theme.t("e.g.", "例如")
                    ),
                    Tone::Faint,
                )],
                (false, Ok(d)) => vec![
                    theme.span("● ", Tone::Ok),
                    theme.span(d.engine.clone(), Tone::Ink),
                    theme.span(
                        fit(
                            &format!("  {}:{} / {} as {}", d.host, d.port, d.database, d.user),
                            usize::from(rw).saturating_sub(12),
                        ),
                        Tone::Dim,
                    ),
                ],
                (false, Err(_)) => vec![theme.span(
                    theme.t(
                        "○ keep typing: scheme://user@host:port/database",
                        "○ 继续输入：scheme://user@host:port/database",
                    ),
                    Tone::Faint,
                )],
            };
            put(buf, rx + 2, ry, rw.saturating_sub(2), hint);
            ry += 1;
        }
        1 => {
            for (index, (en, zh)) in FIELDS.iter().enumerate() {
                let name = theme.t(en, zh);
                if ry >= inner.bottom() {
                    break;
                }
                let on = index == state.field;
                let value = if index == 5 && !state.fields[5].is_empty() {
                    MASK.to_owned()
                } else {
                    state.fields[index].clone()
                };
                let shown = if value.is_empty() && index == 2 {
                    engine_of(state.fields[0].trim())
                        .and_then(default_port)
                        .map_or(String::new(), |p| {
                            format!("{p} ({})", theme.t("default", "默认"))
                        })
                } else {
                    value
                };
                put(
                    buf,
                    rx,
                    ry,
                    rw,
                    vec![
                        theme.span(
                            theme::pad(name, 10),
                            if on { Tone::Accent } else { Tone::Dim },
                        ),
                        theme.span(fit(&shown, usize::from(rw).saturating_sub(12)), Tone::Ink),
                        theme.span(if on { "▏" } else { "" }, Tone::Accent),
                    ],
                );
                ry += 1;
            }
        }
        _ => {
            let lines: [&str; 3] = if theme.zh {
                [
                    "本地 PostgreSQL，内含 OO-Logistics 子集：12 张表，",
                    "一张代码表和一张日历表。只读账号，本地 CA。",
                    "不需要模型 key；示例库自带审核过的本体。",
                ]
            } else {
                [
                    "A local PostgreSQL with an OO-Logistics subset: 12 tables,",
                    "a code table and a calendar. Read-only user, local CA.",
                    "No model key needed; the demo ships a reviewed ontology.",
                ]
            };
            for line in lines {
                put(
                    buf,
                    rx + 2,
                    ry,
                    rw.saturating_sub(2),
                    vec![theme.span(line, Tone::Dim)],
                );
                ry += 1;
            }
        }
    }
    if let Some(error) = &state.error {
        put(
            buf,
            rx + 2,
            ry,
            rw.saturating_sub(2),
            vec![theme.span(format!("✗ {error}"), Tone::Bad)],
        );
    }
    let cy = if wide {
        (inner.y + 7).max(ry + 2)
    } else {
        ry + 2
    };
    if cy + 1 < inner.bottom() && !state.checks.is_empty() {
        heading(
            buf,
            inner.x,
            cy,
            inner.width,
            theme,
            theme.t("read-only checks", "只读检查"),
            theme.t(
                "advice only; oo never runs DBA statements",
                "只给建议；oo 从不执行 DBA 语句",
            ),
        );
        let frame = theme.frame(state.frame);
        let shown = if frame == usize::MAX {
            usize::MAX
        } else {
            frame.saturating_sub(state.checks_at) + 1
        };
        checklist(
            buf,
            Rect::new(
                inner.x,
                cy + 2,
                inner.width,
                inner.bottom().saturating_sub(cy + 2),
            ),
            theme,
            &state.checks,
            state.frame,
            shown,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Status, snapshot};
    use super::*;

    #[test]
    fn parses_dsns_and_detects_engines() {
        let d = parse_dsn(
            "postgres://oo_reader:s%40cret@db.internal:6543/logistics?sslmode=verify-full",
        )
        .unwrap();
        assert_eq!(
            (
                d.engine.as_str(),
                d.host.as_str(),
                d.port,
                d.database.as_str(),
                d.user.as_str()
            ),
            ("postgres", "db.internal", 6543, "logistics", "oo_reader")
        );
        assert_eq!(d.password.as_deref(), Some("s@cret"));
        assert!(!format!("{d:?}").contains("s@cret"));
        assert_eq!(parse_dsn("jdbc:mysql://r@h/db").unwrap().port, 3306);
        assert_eq!(parse_dsn("oracle://r@h/ORCLPDB1").unwrap().port, 1521);
        assert_eq!(parse_dsn("mssql://r@[::1]:1444/db").unwrap().host, "::1");
        assert_eq!(parse_dsn("ftp://h/db").unwrap_err(), "DSN_ENGINE_UNKNOWN");
        assert_eq!(parse_dsn("postgres://r@h:0/db").unwrap_err(), "DSN_PORT");
        assert_eq!(parse_dsn("db.internal").unwrap_err(), "DSN_SCHEME");
    }

    #[test]
    fn masks_passwords_without_revealing_length() {
        assert_eq!(mask("postgres://u:pw@h/db"), "postgres://u:••••••@h/db");
        assert_eq!(
            mask("postgres://u:a-much-longer-password@h/db"),
            "postgres://u:••••••@h/db"
        );
        assert_eq!(mask("postgres://u:partial"), "postgres://u:••••••");
        assert_eq!(mask("postgres://u@h/db"), "postgres://u@h/db");
    }

    pub(crate) fn sample() -> Connect {
        let mut s = Connect::new();
        s.dsn = "postgres://oo_reader:hunter2@db.internal:5432/logistics".into();
        s.refresh();
        s.frame = 3;
        s.checks = vec![
            Check::new(
                "engine",
                Status::Pass,
                "postgres (from the connection string)",
            ),
            Check::new(
                "reachable",
                Status::Pass,
                "db.internal:5432 answered in 12 ms",
            ),
            Check::new(
                "TLS",
                Status::Pass,
                "server certificate verified with your CA",
            ),
            Check::new("read-only account", Status::Running, "checking privileges…"),
            Check::new(
                "CDC capability",
                Status::Warn,
                "wal_level=replica; scheduled refresh still works",
            )
            .fix("ALTER SYSTEM SET wal_level = logical; (run by your DBA)"),
            Check::new("catalog access", Status::Waiting, "after the account check"),
        ];
        s
    }

    #[test]
    fn snapshots() {
        let state = sample();
        snapshot::all_sizes("1-connect", |area, buf, theme| {
            render(&state, area, buf, theme)
        });
        let mut fields = Connect::new();
        fields.mode = 1;
        fields.field = 5;
        fields.fields = [
            "postgres".into(),
            "db.internal".into(),
            String::new(),
            "logistics".into(),
            "oo_reader".into(),
            "hunter2".into(),
            String::new(),
        ];
        snapshot::check(
            "1-connect-fields",
            80,
            24,
            Theme::MONO,
            |area, buf, theme| render(&fields, area, buf, theme),
        );
        let text = {
            let mut buf = Buffer::empty(Rect::new(0, 0, 120, 40));
            render(&state, buf.area, &mut buf, &Theme::MONO);
            render(&fields, Rect::new(0, 0, 120, 40), &mut buf, &Theme::MONO);
            super::super::buffer_text(&buf)
        };
        assert!(!text.contains("hunter2"));
    }

    #[test]
    fn check_rows_reveal_one_a_frame() {
        let mut state = sample();
        state.checks_at = 10;
        let frames: Vec<(usize, &str)> = (10..17)
            .map(|f| (f, "checks arrive, one row a frame"))
            .chain([(usize::MAX, "still")])
            .collect();
        snapshot::frames(
            "1-connect",
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
