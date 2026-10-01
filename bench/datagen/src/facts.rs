//! Seeded business facts. These strings are the scenario the generator must
//! emit. The verifier re-reads source files and recomputes relationships.
//! It does not call the row generator.

pub const PROTOCOL: &str = "oo-datagen/v2";
pub const DATASET: &str = "oo-logistics/v1";
pub const AUTHZ_POLICY: &str = "oo-logistics-authz/v1";
pub const AS_OF: &str = "2026-10-01T00:00:00+08:00";
pub const BUSINESS_TZ: &str = "Asia/Shanghai";
pub const FISCAL_START_MONTH: u8 = 4;
pub const CLOSING_LAG_DAYS: u8 = 5;
pub const EPOCH: u64 = 1;
pub const TRUTH_MARKER: &str = "TRUTH-ONLY-MARKER-9C2E";
pub const PROMPT_CANARY: &str = "OO-CANARY-PROMPT-7F3A";
pub const PII_PHONE: &str = "CANARY-PII-PHONE-0000";
pub const PII_SFZH: &str = "CANARY-PII-SFZH-0000";
pub const BIG_U64: &str = "18446744073709551615";
pub const HIGH_PREC: &str = "1234567890123456789.123456789012";
pub const DST_GAP_LOCAL: &str = "2026-03-08 02:30:00";
pub const DST_OVERLAP_LOCAL: &str = "2026-11-01 01:30:00";
pub const DST_ZONE: &str = "America/New_York";
pub const ZERO_DATE: &str = "0000-00-00";
pub const BAD_TEXT_DATE: &str = "2026-02-30";
pub const SLASH_DATE: &str = "2026/1/5";
pub const PLACEHOLDER_TAX: &str = "000000000000000000";
pub const TAX_15: &str = "310101234567890";
pub const TAX_HX_BODY: &str = "91350100M000100Y4";
pub const NAME_HX: &str = "浣熊市警察局";
pub const NAME_HX_UNIT: &str = "浣熊市警察局后勤中心";
pub const ALIAS_RPD: &str = "RPD";
pub const ALIAS_SHORT: &str = "浣熊警局";
pub const NAME_SIM_A: &str = "北港钢铁一厂";
pub const NAME_SIM_B: &str = "北港钢铁贸易";
pub const NAME_NULL_TAX: &str = "合成客户-税号空";
pub const NAME_PLACEHOLDER: &str = "合成客户-税号占位";
pub const NAME_TAX15: &str = "合成客户-十五位税号";
pub const NAME_BADTAX: &str = "合成客户-税号校验错";
pub const NAME_TEST: &str = "TEST-调度演练客户";
pub const NAME_TEST_OFF: &str = "合成客户-演练";
pub const NAME_SOFT: &str = "合成客户-已软删除";
pub const NAME_SOFT_OFF: &str = "合成客户-在册";
pub const NAME_EMOJI_OFF: &str = "合成客户-港口";
pub const NAME_ZT0: &str = "合成客户-状态零";
pub const NAME_OLD: &str = "合成客户-旧冻结";
pub const NAME_NEW: &str = "合成客户-新停用";
pub const NAME_ZERO: &str = "合成客户-零日期";
pub const NAME_EMOJI: &str = "合成客户-港口-🚚";
pub const NAME_PROMPT: &str = "合成客户-备注注入";
pub const NAME_HIPREC: &str = "合成客户-高精度";
pub const NAME_U64: &str = "合成客户-大整数";
pub const NAME_USD: &str = "合成客户-美元";
pub const NAME_EUR: &str = "合成客户-欧元";
pub const NAME_WEST: &str = "西丘联运合成户";
pub const NAME_GAP: &str = "合成客户-字典缺码";
pub const CONTRACT_HX: &str = "HT-HUANXIONG-2026";
pub const ORDER_REF: &str = "REF-HX-001";
pub const MISMATCH_REF: &str = "REF-MISMATCH-001";
pub const HX_LINES_FEN: [i64; 3] = [10_000, 2_500, 5_050];
pub const HX_ORDER_FEN: i64 = 17_550;
pub const HX_INVOICE_FEN: i64 = 17_550;
pub const HX_INVOICE_DATE: &str = "2026-03-18";
pub const HX_RECEIPT_MARCH_FEN: i64 = 7_550;
pub const HX_RECEIPT_MARCH_DATE: &str = "2026-03-28";
pub const HX_RECEIPT_APRIL_FEN: i64 = 10_000;
pub const HX_RECEIPT_APRIL_DATE: &str = "2026-04-02";
pub const GBK_HX_HEX: &str = "e4bdd0dccad0beafb2ecbed6";
pub const GB18030_RARE_HEX: &str = "95328236";
pub const LARGE_TABLE_ROWS: u64 = 1_000;

pub const SMELLS: &[&str] = &[
    "tinyint_012",
    "zero_date",
    "text_date",
    "unsigned64_above_i64",
    "high_precision_numeric",
    "cents",
    "multi_currency",
    "enum_set",
    "emoji",
    "gbk_bytes",
    "gb18030_bytes",
    "timezone_mismatch",
    "dst_gap",
    "dst_overlap",
    "soft_delete",
    "test_customer",
    "dictionary_gap",
    "code_meaning_by_time",
    "alias_duplicate_customer",
    "tax_id_null",
    "tax_id_placeholder",
    "tax_id_15",
    "tax_id_bad_checksum",
    "similar_not_merge",
    "metric_invoice_vs_receipt",
    "fiscal_calendar",
    "closing_period",
    "multi_table_txn",
    "month_end_txn",
    "dangling_ref",
    "amount_mismatch",
    "prompt_injection",
];

pub fn smell_known(name: &str) -> bool {
    SMELLS.contains(&name)
}
