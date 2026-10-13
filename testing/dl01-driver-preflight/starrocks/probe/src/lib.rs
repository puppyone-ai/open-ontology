#![forbid(unsafe_code)]

pub mod audit;
pub mod civil;
pub mod decimal;
pub mod encode;
pub mod fail;
pub mod identity;
pub mod plan;
pub mod session;

pub const PROTOCOL: &str = "oo.source.starrocks/v1";
pub const EXPECTED_BUILD: &str = "3.5.21-85fccf8";
pub const DATABASE: &str = "oo_w45_sr";
pub const READER_USER: &str = "oo_w45_reader";
