//! Configuration wizard. A saved configuration is not a deployed/reviewed pack.
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use oo_stack::secret::SecretStore;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::{Block, Paragraph, Wrap};
use serde_json::{Value, json};

use super::{Failure, Invocation, doctor};

struct Screen {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl Screen {
    fn open() -> Result<Self, Failure> {
        enable_raw_mode().map_err(|_| Failure::local("TUI_TERMINAL"))?;
        if execute!(io::stdout(), EnterAlternateScreen).is_err() {
            let _ = disable_raw_mode();
            return Err(Failure::local("TUI_TERMINAL"));
        }
        match Terminal::new(CrosstermBackend::new(io::stdout())) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(_) => {
                let _ = execute!(io::stdout(), LeaveAlternateScreen);
                let _ = disable_raw_mode();
                Err(Failure::local("TUI_TERMINAL"))
            }
        }
    }

    fn prompt(
        &mut self,
        step: u8,
        label: &str,
        default: &str,
        secret: bool,
    ) -> Result<String, Failure> {
        let mut value = String::new();
        let mut scroll = 0u16;
        loop {
            // Secret contents and lengths never enter a render buffer.
            let shown = if secret { "[hidden]" } else { value.as_str() };
            let hint = if secret || default.is_empty() {
                String::new()
            } else {
                format!("\nDefault: {default}")
            };
            let text = format!(
                "{label}{hint}\n\n> {shown}\n\nEnter: accept   Esc / Ctrl-C: cancel   Up/Down: scroll\n\
                 Credentials already entered remain saved on cancellation."
            );
            self.terminal
                .draw(|frame| {
                    frame.render_widget(
                        Paragraph::new(text)
                            .wrap(Wrap { trim: false })
                            .scroll((scroll, 0))
                            .block(
                                Block::bordered()
                                    .title(format!("Open Ontology / step {step} of 7")),
                            ),
                        frame.area(),
                    );
                })
                .map_err(|_| Failure::local("TUI_RENDER"))?;
            match event::read().map_err(|_| Failure::local("TUI_INPUT"))? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if key.code == KeyCode::Esc
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        return Err(Failure::local("TUI_CANCELLED"));
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
                _ => (),
            }
        }
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

fn secret(
    screen: &mut Screen,
    store: &SecretStore,
    step: u8,
    label: &str,
    default: &str,
) -> Result<Value, Failure> {
    let name = screen.prompt(
        step,
        &format!("{label} secret name (new file; never overwritten)"),
        default,
        false,
    )?;
    if !oo_stack::stack::secret_name(&name) {
        return Err(Failure::usage("SECRET_NAME"));
    }
    let value = screen.prompt(
        step,
        &format!("{label} (stored immediately, never displayed)"),
        "",
        true,
    )?;
    store
        .write_new(&name, value.as_bytes())
        .map_err(Failure::local)?;
    Ok(json!({"secret_ref":name}))
}

fn list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn save_new(stack: &oo_stack::Stack, path: &Path) -> Result<(), Failure> {
    // create_new refuses existing files and symlinks. An aborted wizard never
    // replaces a configuration or deletes credentials the user may have kept.
    let mut bytes = serde_json::to_vec_pretty(stack).map_err(|_| Failure::local("TUI_CONFIG"))?;
    bytes.push(b'\n');
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| Failure::local("TUI_CONFIG_EXISTS_OR_IO"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| Failure::local("TUI_CONFIG_IO"))
}

pub(super) fn run() -> Result<Invocation, Failure> {
    if Path::new("stack.yaml").symlink_metadata().is_ok() {
        return Err(Failure::local("TUI_CONFIG_EXISTS_OR_IO"));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| Failure::local("SECRET_HOME"))?;
    let parent = Path::new(&home).join(".config/open-ontology");
    fs::create_dir_all(&parent).map_err(|_| Failure::local("SECRET_DIRECTORY"))?;
    let store = SecretStore::create(parent.join("secrets")).map_err(Failure::local)?;
    let mut screen = Screen::open()?;
    let engine = screen.prompt(
        1,
        "Engine: postgres / oracle / mysql (unavailable)",
        "postgres",
        false,
    )?;
    let mut source = json!({
        "id": screen.prompt(1, "Source ID", "source", false)?,
        "engine": engine,
        "host": screen.prompt(1, "Source host", "localhost", false)?,
        "port": screen.prompt(1, "Source port", match engine.as_str() {
            "oracle" => "1521", "mysql" => "3306", _ => "5432"
        }, false)?.parse::<u16>().map_err(|_| Failure::usage("STACK_SOURCE_PORT"))?,
        "database": screen.prompt(1, "Database or Oracle service", "", false)?,
        "user": screen.prompt(1, "Read-only user", "", false)?,
        "password": secret(&mut screen, &store, 1, "Source password", "source-password")?,
    });
    let ca = screen.prompt(
        1,
        "TLS CA secret_ref (existing store file; required for PostgreSQL)",
        "",
        false,
    )?;
    if !ca.is_empty() {
        source["tls_ca"] = json!({"secret_ref":ca});
    }
    let ssh = screen.prompt(
        1,
        "SSH jump host (blank: direct; transport not yet available)",
        "",
        false,
    )?;
    if !ssh.is_empty() {
        source["ssh"] = json!({
            "host":ssh, "user":screen.prompt(1, "SSH user", "", false)?,
            "key":{"secret_ref":screen.prompt(1, "SSH key secret_ref (existing file)", "", false)?},
            "host_key_sha256":screen.prompt(1, "Verified SSH host key SHA256 fingerprint", "", false)?,
        });
    }
    source["scope"] = json!({
        "include":list(&screen.prompt(2, "Include schema.table entries, comma separated (wildcard discovery not yet available)", "*", false)?),
        "exclude":list(&screen.prompt(2, "Exclude tables or schema.*, comma separated", "", false)?),
    });
    let model = json!({
        "provider":screen.prompt(3, "Provider: openai-compatible / anthropic", "openai-compatible", false)?,
        "base_url":screen.prompt(3, "Model base URL", "", false)?,
        "model":screen.prompt(3, "Model ID", "", false)?,
        "key":secret(&mut screen, &store, 3, "Model key", "model-key")?,
        "context_window":"auto",
    });
    screen.prompt(4, "Deployment: local Docker / lite. Remote SSH deployment is not yet available. Enter to continue.", "", false)?;
    let stack = oo_stack::parse(
        &json!({
            "version":"oo.stack/v1", "sources":[source], "model":model,
            "deploy":{"target":"local","profile":"lite"},
            "gateway":{"public_url":"auto"},
        })
        .to_string(),
    )
    .map_err(Failure::usage)?;
    screen.prompt(
        5,
        "Run read-only doctor and bounded model probe (two minimal completions). Enter to run.",
        "",
        false,
    )?;
    let preflight = doctor::run(&stack, true)?;
    let (report, exit) = match preflight {
        Invocation::AdminReport(report, exit) => (report, exit),
        _ => return Err(Failure::local("TUI_PREFLIGHT_PROTOCOL")),
    };
    screen.prompt(5, &format!("Preflight (exit {exit}):\n{report}\n\nEnter to continue. Blocked preflight never deploys."), "", false)?;
    let confirm = screen.prompt(6,
        "Save stack.yaml? Type SAVE. This saves configuration ONLY; pull/build/review/publication are not yet wired. Credentials already saved remain on cancellation.",
        "", false)?;
    if confirm != "SAVE" {
        return Err(Failure::local("TUI_CANCELLED"));
    }
    save_new(&stack, Path::new("stack.yaml"))?;
    screen.prompt(7,
        "Configuration saved (0600). No services started. No draft, Pack, Gateway URL or token issued. Enter to exit.",
        "", false)?;
    Ok(Invocation::Help(
        "Stack saved; no services started or publication performed.".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_bytes_match_noninteractive_normalization_and_never_overwrite() {
        let dir = std::env::temp_dir().join(format!("oo-wizard-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let config = oo_stack::parse(include_str!("../../../oo-stack/example.stack.json")).unwrap();
        let wizard = dir.join("wizard");
        let noninteractive = dir.join("noninteractive");
        assert!(save_new(&config, &wizard).is_ok());
        fs::write(
            &noninteractive,
            include_str!("../../../oo-stack/example.stack.json"),
        )
        .unwrap();
        assert!(super::super::normalize(&noninteractive).is_ok());
        assert_eq!(
            fs::read(&wizard).unwrap(),
            fs::read(&noninteractive).unwrap()
        );
        assert!(save_new(&config, &wizard).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
