pub const MAX_SOURCE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_CSV_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_SIDECAR_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_REQUEST_BYTES: usize = 262_144;
pub const MAX_ROWS: usize = 2000;
pub const MAX_COLUMNS: usize = 512;
pub const MAX_COLUMN_NAME_BYTES: usize = 256;
pub const MAX_JSON_DEPTH: usize = 8;
pub const MAX_JSON_NODES: usize = 8192;

pub const SOURCE_NAME: &str = "export.source.jsonl";
pub const CSV_NAME: &str = "export.csv";
pub const SIDECAR_NAME: &str = "export.sidecar.json";
pub const TRANSFORMATION: &str = "apostrophe-prefix-after-leading-whitespace-or-control";
