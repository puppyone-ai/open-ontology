#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fault {
    pub token: &'static str,
    pub exit_code: u8,
}

impl Fault {
    pub const fn new(token: &'static str, exit_code: u8) -> Self {
        Self { token, exit_code }
    }

    pub const fn provider() -> Self {
        Self::new("provider", 1)
    }

    pub const fn secret() -> Self {
        Self::new("secret", 1)
    }

    pub const fn budget() -> Self {
        Self::new("budget", 1)
    }

    pub const fn io() -> Self {
        Self::new("io", 1)
    }

    pub const fn usage() -> Self {
        Self::new("usage", 2)
    }

    pub const fn config() -> Self {
        Self::new("config", 3)
    }

    pub const fn pack() -> Self {
        Self::new("pack", 4)
    }

    pub const fn approval() -> Self {
        Self::new("approval", 4)
    }

    pub const fn binary() -> Self {
        Self::new("binary", 5)
    }

    pub const fn guard() -> Self {
        Self::new("guard", 6)
    }

    pub const fn query() -> Self {
        Self::new("query", 7)
    }

    pub const fn citations() -> Self {
        Self::new("citations", 8)
    }
}
