use std::fmt;

#[derive(Debug)]
pub struct Error {
    pub kind: &'static str,
    pub state: Option<String>,
    pub detail: String,
}

impl Error {
    pub fn new(kind: &'static str) -> Self {
        Self {
            kind,
            state: None,
            detail: String::new(),
        }
    }

    pub fn sql(err: tokio_postgres::Error) -> Self {
        let state = err.code().map(|code| code.code().to_string());
        let mut detail = match err.as_db_error() {
            Some(db) => {
                let mut text = db.message().to_string();
                if let Some(extra) = db.detail() {
                    text.push(' ');
                    text.push_str(extra);
                }
                text
            }
            None => err.to_string(),
        };
        detail.retain(|ch| ch != '\n' && ch != '\r');
        if detail.len() > 180 {
            detail.truncate(180);
        }
        Self {
            kind: "sql",
            state,
            detail,
        }
    }

    pub fn is_unique(&self) -> bool {
        self.state.as_deref() == Some("23505")
    }

    pub fn is_deadlock(&self) -> bool {
        self.state.as_deref() == Some("40P01")
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.state {
            Some(state) => write!(f, "{} sqlstate={state} {}", self.kind, self.detail),
            None => write!(f, "{} {}", self.kind, self.detail),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
