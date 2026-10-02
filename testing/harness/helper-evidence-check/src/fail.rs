#[derive(Debug)]
pub struct Fail {
    pub code: &'static str,
    pub exit_code: i32,
    pub detail: String,
}

pub fn fail(code: &'static str, exit_code: i32, detail: impl Into<String>) -> Fail {
    let mut detail = detail.into().replace(['\n', '\r'], " ");
    if detail.len() > 180 {
        detail.truncate(180);
    }
    Fail {
        code,
        exit_code,
        detail,
    }
}

pub const USAGE: i32 = 1;
pub const MISSING_LICENSE_BODY: i32 = 2;
pub const HASH_MISMATCH: i32 = 3;
pub const WRONG_DUAL_CHOICE: i32 = 4;
pub const UNDECLARED_CHOICE: i32 = 5;
pub const OMITTED_APACHE_BODY: i32 = 6;
pub const MISSING_THIRD_PARTY_NOTICE: i32 = 7;
pub const INVALID_TIME: i32 = 8;
pub const AGE_FLOOR: i32 = 9;
pub const HELPER_LOCK_CHANGED: i32 = 10;
pub const SCANNER_OUTPUT_MISSING: i32 = 11;
pub const PATH_TRAVERSAL: i32 = 12;
pub const UNKNOWN_PACKAGE: i32 = 13;
pub const DUPLICATE: i32 = 14;
pub const WRONG_SOURCE: i32 = 15;
pub const WRONG_EXPRESSION: i32 = 16;
pub const PLACEHOLDER: i32 = 17;
pub const CHECKSUM_MISMATCH: i32 = 18;
pub const FAKE_CLAIM: i32 = 19;
pub const GRAPH_MISMATCH: i32 = 20;
pub const MALFORMED: i32 = 21;
pub const BOUND: i32 = 22;
