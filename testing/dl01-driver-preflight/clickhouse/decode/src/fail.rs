use std::fmt;

#[derive(Debug)]
pub struct Fail(pub String);

impl fmt::Display for Fail {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Fail {}

pub type Gate<T> = Result<T, Fail>;

pub fn fail(text: impl Into<String>) -> Fail {
    Fail(text.into())
}
