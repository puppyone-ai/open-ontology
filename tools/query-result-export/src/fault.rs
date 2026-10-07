#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault {
    pub token: &'static str,
    pub code: u8,
}

impl Fault {
    pub const fn io() -> Self {
        Self { token: "io", code: 1 }
    }

    pub const fn usage() -> Self {
        Self {
            token: "usage",
            code: 2,
        }
    }

    pub const fn request() -> Self {
        Self {
            token: "request",
            code: 3,
        }
    }

    pub const fn budget() -> Self {
        Self {
            token: "budget",
            code: 3,
        }
    }

    pub const fn path() -> Self {
        Self { token: "path", code: 4 }
    }

    pub const fn source() -> Self {
        Self {
            token: "source",
            code: 5,
        }
    }

    pub fn exit(self) -> std::process::ExitCode {
        let token = self.token;
        eprintln!("query-result-export: {token}");
        std::process::ExitCode::from(self.code)
    }
}
