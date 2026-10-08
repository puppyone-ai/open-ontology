#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault {
    pub token: &'static str,
    pub code: u8,
}

impl Fault {
    pub const fn io() -> Self {
        Self { token: "io", code: 1 }
    }

    pub const fn uncited() -> Self {
        Self {
            token: "uncited",
            code: 1,
        }
    }

    pub const fn canceled() -> Self {
        Self {
            token: "canceled",
            code: 1,
        }
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

    pub const fn engine() -> Self {
        Self {
            token: "engine",
            code: 3,
        }
    }

    pub const fn xlsx() -> Self {
        Self {
            token: "xlsx",
            code: 3,
        }
    }

    pub const fn parquet() -> Self {
        Self {
            token: "parquet",
            code: 3,
        }
    }

    pub const fn path() -> Self {
        Self { token: "path", code: 4 }
    }

    pub const fn hash() -> Self {
        Self { token: "hash", code: 4 }
    }

    pub const fn lineage() -> Self {
        Self {
            token: "lineage",
            code: 4,
        }
    }

    pub const fn source() -> Self {
        Self {
            token: "source",
            code: 5,
        }
    }

    pub fn exit(self) -> std::process::ExitCode {
        let token = self.token;
        eprintln!("local-analysis: {token}");
        std::process::ExitCode::from(self.code)
    }
}
