use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    pub status: u16,
    pub reason: &'static str,
    pub exit_code: u8,
}

impl Fault {
    pub const fn new(status: u16, reason: &'static str, exit_code: u8) -> Self {
        Self { status, reason, exit_code }
    }

    pub const fn public(&self) -> &'static str {
        self.reason
    }

    pub const fn exit_code(&self) -> u8 {
        self.exit_code
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason)
    }
}

pub fn unauthenticated() -> Fault {
    Fault::new(401, "unauthenticated", 2)
}

pub fn csrf() -> Fault {
    Fault::new(403, "csrf", 2)
}

pub fn unmapped() -> Fault {
    Fault::new(403, "data_role_unmapped", 2)
}

pub fn malformed(reason: &'static str) -> Fault {
    Fault::new(422, reason, 1)
}

pub fn conflict(reason: &'static str) -> Fault {
    Fault::new(409, reason, 2)
}

pub fn unavailable(reason: &'static str) -> Fault {
    Fault::new(503, reason, 3)
}

pub fn not_found() -> Fault {
    Fault::new(404, "not_found", 2)
}
