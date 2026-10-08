#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault {
    pub token: &'static str,
    pub code: u8,
}

impl Fault {
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

    pub const fn binding() -> Self {
        Self {
            token: "binding",
            code: 4,
        }
    }

    pub const fn hash() -> Self {
        Self {
            token: "hash",
            code: 4,
        }
    }

    pub const fn receipt() -> Self {
        Self {
            token: "receipt",
            code: 5,
        }
    }

    pub fn exit(self) -> std::process::ExitCode {
        let token = self.token;
        eprintln!("answer-citations: {token}");
        std::process::ExitCode::from(self.code)
    }
}
