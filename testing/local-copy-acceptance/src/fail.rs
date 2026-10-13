#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fail {
    pub code: &'static str,
    pub detail: &'static str,
}

pub fn fail(code: &'static str, detail: &'static str) -> Fail {
    Fail { code, detail }
}

impl Fail {
    pub fn exit_code(&self) -> u8 {
        match self.code {
            "usage" => 1,
            _ => 3,
        }
    }
}
