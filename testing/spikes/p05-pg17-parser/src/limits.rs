use std::time::Duration;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_SQL: usize = 64 * 1024;
pub const MAX_FRAME_BODY: usize = 256 * 1024;
pub const MAX_NODES: u32 = 10_000;
pub const MAX_DEPTH: u32 = 128;
pub const PARSER_VERSION_NUM: i32 = 170007;
pub const PARSE_DEADLINE: Duration = Duration::from_millis(500);
pub const SPAWN_BUDGET: Duration = Duration::from_secs(5);
