pub const MAX_REQUEST_BYTES: usize = 262_144;
pub const MAX_LINEAGE_BYTES: usize = 262_144;
pub const MAX_EXPORT_BYTES: usize = 1_048_576;
pub const MAX_JSON_DEPTH: usize = 8;
pub const MAX_JSON_NODES: usize = 400_000;
pub const MAX_ROWS: usize = 2000;
pub const MAX_COLUMNS: usize = 512;
pub const MAX_GROUPS: usize = 64;
pub const MAX_LABEL_BYTES: usize = 256;
pub const MAX_CELL_BYTES: usize = 65_536;
pub const MAX_COEFFICIENT_DIGITS: usize = 8192;
pub const MAX_SUM_DIGITS: usize = 16_384;
pub const MAX_OUTPUT_BYTES: usize = 65_536;
pub const MAX_RECEIPT_LINE: usize = 4096;

pub const TRANSFORMATION: &str = "apostrophe-prefix-after-leading-whitespace-or-control";

pub const SOURCE_NAME: &str = "export.source.jsonl";
pub const CSV_NAME: &str = "export.csv";
pub const SIDECAR_NAME: &str = "export.sidecar.json";

pub const OUT_CSV: &str = "analysis.csv";
pub const OUT_SVG: &str = "analysis.svg";
pub const OUT_MANIFEST: &str = "analysis.manifest.json";
pub const OUT_PARQUET: &str = "analysis.parquet";
pub const OUT_ARROW: &str = "analysis.arrow";
pub const OUT_XLSX: &str = "analysis.xlsx";
pub const OUT_FONT: &str = "NotoSansCJKsc-Regular.otf";

pub const MAX_CLI_BYTES: usize = 100_000_000;
pub const MAX_FONT_BYTES: usize = 64_000_000;
