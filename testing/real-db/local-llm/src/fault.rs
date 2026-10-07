use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault {
    token: &'static str,
}

impl Fault {
    pub const fn new(token: &'static str) -> Self {
        Self { token }
    }

    pub const fn token(self) -> &'static str {
        self.token
    }

    pub const fn config() -> Self {
        Self::new("config")
    }

    pub const fn bind() -> Self {
        Self::new("bind")
    }

    pub const fn tls() -> Self {
        Self::new("tls")
    }

    pub const fn io() -> Self {
        Self::new("io")
    }

    pub const fn path() -> Self {
        Self::new("path")
    }

    pub const fn request_budget() -> Self {
        Self::new("request_budget")
    }

    pub const fn response_budget() -> Self {
        Self::new("response_budget")
    }

    pub const fn busy() -> Self {
        Self::new("busy")
    }

    pub const fn upstream() -> Self {
        Self::new("upstream")
    }

    pub const fn secret() -> Self {
        Self::new("secret")
    }

    pub const fn close() -> Self {
        Self::new("close")
    }

    pub const fn client_token() -> Self {
        Self::new("client_token")
    }

    pub fn http_status(self) -> u16 {
        match self.token {
            "path" => 404,
            "request_budget" => 413,
            "busy" => 503,
            "client_token" => 401,
            "response_budget" | "upstream" => 502,
            _ => 400,
        }
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.token)
    }
}
