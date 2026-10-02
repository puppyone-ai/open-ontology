use std::time::Duration;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_SQL: usize = 64 * 1024;
pub const MAX_FRAME_BODY: usize = 256 * 1024;
pub const MAX_NODES: u32 = 10_000;
pub const MAX_DEPTH: u32 = 128;
pub const PARSER_VERSION_NUM: i32 = 170007;
pub const PARSE_DEADLINE: Duration = Duration::from_millis(500);
pub const SPAWN_BUDGET: Duration = Duration::from_secs(5);
/// Separate from readiness and parse. A cleanup that cannot finish inside this
/// ceiling is not success.
pub const CLEANUP_BUDGET: Duration = Duration::from_millis(500);
pub const MAX_DETAIL: usize = 2048;
/// Real cap on the worker image we will copy into a sealed memfd.
pub const MAX_WORKER_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_STDOUT_BYTES: usize = MAX_FRAME_BODY + 64;
/// Bytes the worker may create in its temp directory. Enforced with RLIMIT_FSIZE
/// and checked again on the directory after the process is gone.
pub const WORKDIR_BYTE_BUDGET: u64 = 256 * 1024;
pub const WORKER_FSIZE: u64 = 256 * 1024;
pub const WORKER_ADDRESS_SPACE: u64 = 1024 * 1024 * 1024;
pub const WORKER_NOFILE: u64 = 64;
/// Runaway cap for the worker. The parse deadline is the wall-clock control.
/// Debug builds of the wide corpus can spend more than a couple of CPU seconds
/// before they return a deny, so this is wider than the 500ms parse budget.
pub const WORKER_CPU_SEC: u64 = 15;
