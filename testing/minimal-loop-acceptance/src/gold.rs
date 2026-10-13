//! Independent gold for the sealed bulk import and the older shop fixture.
//!
//! The frozen prompts are the bulk questions. They contain no answer and no SQL.
//! Shop formulas stay for the shop fixture. They are not the sealed-import questions.

pub const PARTY_ROWS: i64 = 2_000;
pub const ACCOUNT_ROWS: i64 = 8_000;
pub const ENTRY_ROWS: i64 = 90_000;
pub const PI_COMMIT: &str = "7c10bd4337495ee613f2224843ecdf349b80d1df";
pub const DEVELOPMENT_ACK: &str = "local-process-not-a-sandbox-certificate";
pub const MODEL_ID: &str = "grok-4.7";

#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Null,
    Bool(bool),
    Text(String),
}

#[derive(Clone, Copy, Debug)]
pub struct FrozenQuestion {
    pub id: &'static str,
    pub text: &'static str,
}

pub const BULK_IMPORT: &str = "49dbba9719dadd3b7a95b4376d9b47b171ab7ae0ba33623df75d91c5ac85db73";
pub const BULK_SNAPSHOT: &str = "00000004-00000013-1";
pub const BULK_COUNT_C: &str = "90000";
pub const BULK_ID_OVERLAP: &str = "2000";
pub const BULK_AMOUNT_SUM: &str = "134997.0000";
pub const BULK_NULL_AMOUNT: &str = "1";
pub const BULK_AMOUNT_EXACT: &str = "1234567890123456789012345678901234.1234";
pub const BULK_QTY_MAX: &str = "9223372036854775807";
pub const BULK_QTY_MIN: &str = "-9223372036854775808";

pub fn frozen_questions() -> &'static [FrozenQuestion] {
    &[
        FrozenQuestion {
            id: "count-batch-c",
            text: "How many rows are in public.batch_c? Write the SQL yourself and run it with oo query.",
        },
        FrozenQuestion {
            id: "id-overlap",
            text: "How many public.batch_a ids also appear in public.batch_c? There is no foreign key between these tables. Write the SQL yourself and run it with oo query.",
        },
        FrozenQuestion {
            id: "amount-when-qty-above",
            text: "What is the exact sum of public.batch_c.amount for rows whose qty is greater than 2? Keep the exact numeric text. Write the SQL yourself and run it with oo query.",
        },
        FrozenQuestion {
            id: "null-amount",
            text: "How many public.batch_a rows have a null amount? Write the SQL yourself and run it with oo query.",
        },
        FrozenQuestion {
            id: "batch-a-amount",
            text: "Return the amount of the public.batch_a row whose id is 2. Keep the exact numeric text. Write the SQL yourself and run it with oo query.",
        },
        FrozenQuestion {
            id: "batch-c-qty-bounds",
            text: "Return id and qty for the public.batch_c rows whose id is 1 or 2, ordered by id. Write the SQL yourself and run it with oo query.",
        },
    ]
}

pub fn shop_questions() -> &'static [FrozenQuestion] {
    &[
        FrozenQuestion {
            id: "count-entry",
            text: "How many rows are in shop.entry? Write the SQL yourself and run it with oo query.",
        },
        FrozenQuestion {
            id: "fk-account",
            text: "What is the party key of shop.account row 0? Write the SQL yourself and run it with oo query.",
        },
        FrozenQuestion {
            id: "null-party",
            text: "For shop.party key p00000, return the key, the display name, and the active flag. Write the SQL yourself.",
        },
        FrozenQuestion {
            id: "qty-bounds",
            text: "Return entry id and quantity for the first three shop.entry rows, ordered by entry id. Write the SQL yourself.",
        },
        FrozenQuestion {
            id: "amount-exact",
            text: "Return the amount of shop.entry row 3. Write the SQL yourself and keep the exact numeric text.",
        },
        FrozenQuestion {
            id: "null-display-count",
            text: "How many shop.party rows have a null display name? Write the SQL yourself.",
        },
    ]
}

pub fn sealed_prompts() -> bool {
    BULK_IMPORT.len() == 64
        && !BULK_SNAPSHOT.is_empty()
        && frozen_questions().iter().chain(shop_questions()).all(|question| !question.text.is_empty() && !prompt_leaks(question.text))
}

pub fn prompt_leaks(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("select") || text.contains("90000") || text.contains("1234567890")
}

pub fn party_key(index: u32) -> String {
    format!("p{index:05}")
}

pub fn party_display(index: u32) -> Option<String> {
    if index.is_multiple_of(17) { None } else { Some(format!("n{index:05}")) }
}

pub fn party_active(index: u32) -> bool {
    index.is_multiple_of(2)
}

pub fn account_party(index: u32) -> String {
    party_key(index % 2_000)
}

pub fn entry_qty(index: u32) -> i64 {
    match index {
        0 => i64::MIN,
        1 => i64::MAX,
        2 => 0,
        _ => i64::from(index) - 45_000,
    }
}

pub fn entry_amount_minor(index: u32) -> Option<i64> {
    match index {
        3 => Some(12_345_678_901_234),
        4 => Some(-12_345_678_901_234),
        5 => Some(1),
        6 => Some(-1),
        _ if index.is_multiple_of(29) => None,
        _ => Some(i64::from(index % 10_000) - 5_000),
    }
}

pub fn format_minor(minor: i64) -> String {
    let negative = minor < 0;
    let absolute = minor.unsigned_abs();
    let whole = absolute / 10_000;
    let frac = absolute % 10_000;
    if negative { format!("-{whole}.{frac:04}") } else { format!("{whole}.{frac:04}") }
}

pub fn null_display_count() -> usize {
    (0..2_000).filter(|index| party_display(*index).is_none()).count()
}

pub fn expected_cells(id: &str) -> Option<Vec<Vec<Cell>>> {
    match id {
        "count-batch-c" => Some(vec![vec![Cell::Text(BULK_COUNT_C.to_owned())]]),
        "id-overlap" => Some(vec![vec![Cell::Text(BULK_ID_OVERLAP.to_owned())]]),
        "amount-when-qty-above" => Some(vec![vec![Cell::Text(BULK_AMOUNT_SUM.to_owned())]]),
        "null-amount" => Some(vec![vec![Cell::Text(BULK_NULL_AMOUNT.to_owned())]]),
        "batch-a-amount" => Some(vec![vec![Cell::Text(BULK_AMOUNT_EXACT.to_owned())]]),
        "batch-c-qty-bounds" => Some(vec![
            vec![Cell::Text("1".to_owned()), Cell::Text(BULK_QTY_MAX.to_owned())],
            vec![Cell::Text("2".to_owned()), Cell::Text(BULK_QTY_MIN.to_owned())],
        ]),
        "count-entry" => Some(vec![vec![Cell::Text(ENTRY_ROWS.to_string())]]),
        "fk-account" => Some(vec![vec![Cell::Text("0".to_owned()), Cell::Text(account_party(0))]]),
        "null-party" => Some(vec![vec![
            Cell::Text(party_key(0)),
            match party_display(0) {
                Some(text) => Cell::Text(text),
                None => Cell::Null,
            },
            Cell::Bool(party_active(0)),
        ]]),
        "qty-bounds" => Some(
            (0..3)
                .map(|index| vec![Cell::Text(index.to_string()), Cell::Text(entry_qty(index).to_string())])
                .collect(),
        ),
        "amount-exact" => {
            let minor = entry_amount_minor(3)?;
            Some(vec![vec![Cell::Text(format_minor(minor))]])
        }
        "null-display-count" => Some(vec![vec![Cell::Text(null_display_count().to_string())]]),
        _ => None,
    }
}

pub fn false_success(status: &str, exit: Option<i32>, http: Option<i64>) -> bool {
    exit == Some(0) || matches!(status, "matched" | "validated" | "draft_ready" | "success") || http == Some(200)
}

pub fn party_phone(index: u32) -> String {
    format!("tel-{index:05}")
}

pub fn party_national_id(index: u32) -> String {
    format!("nid-{index:05}")
}

pub fn account_opened(index: u32) -> bool {
    !index.is_multiple_of(3)
}

pub fn account_label(index: u32) -> Option<String> {
    if index.is_multiple_of(19) { None } else { Some(format!("a{index}")) }
}

pub fn entry_account(index: u32) -> i64 {
    i64::from(index % 8_000)
}

pub fn entry_memo(index: u32) -> Option<String> {
    if index.is_multiple_of(13) { None } else { Some(format!("m{index}")) }
}

pub fn schema_sql() -> &'static str {
    "CREATE SCHEMA shop;\n\
CREATE TABLE shop.party (\n\
  party_key text PRIMARY KEY,\n\
  display_name text,\n\
  active boolean NOT NULL,\n\
  phone text NOT NULL,\n\
  national_id text NOT NULL\n\
);\n\
CREATE TABLE shop.account (\n\
  account_id int8 PRIMARY KEY,\n\
  party_key text NOT NULL,\n\
  opened boolean NOT NULL,\n\
  label text,\n\
  CONSTRAINT account_party_fk FOREIGN KEY (party_key) REFERENCES shop.party (party_key)\n\
);\n\
CREATE TABLE shop.entry (\n\
  entry_id int8 PRIMARY KEY,\n\
  account_id int8 NOT NULL,\n\
  qty int8 NOT NULL,\n\
  amount numeric(18,4),\n\
  memo text,\n\
  CONSTRAINT entry_account_fk FOREIGN KEY (account_id) REFERENCES shop.account (account_id)\n\
);\n\
REVOKE ALL ON SCHEMA shop FROM PUBLIC;\n\
REVOKE ALL ON ALL TABLES IN SCHEMA shop FROM PUBLIC;\n"
}

fn copy_field(out: &mut String, value: Option<&str>) {
    match value {
        None => out.push_str("\\N"),
        Some(text) => out.push_str(text),
    }
}

fn push_bool(out: &mut String, value: bool) {
    out.push(if value { 't' } else { 'f' });
}

pub fn party_line(index: u32) -> String {
    let key = party_key(index);
    let display = party_display(index);
    let phone = party_phone(index);
    let national = party_national_id(index);
    let mut line = String::new();
    copy_field(&mut line, Some(&key));
    line.push('\t');
    copy_field(&mut line, display.as_deref());
    line.push('\t');
    push_bool(&mut line, party_active(index));
    line.push('\t');
    copy_field(&mut line, Some(&phone));
    line.push('\t');
    copy_field(&mut line, Some(&national));
    line.push('\n');
    line
}

pub fn account_line(index: u32) -> String {
    let key = account_party(index);
    let label = account_label(index);
    let mut line = String::new();
    line.push_str(&index.to_string());
    line.push('\t');
    copy_field(&mut line, Some(&key));
    line.push('\t');
    push_bool(&mut line, account_opened(index));
    line.push('\t');
    copy_field(&mut line, label.as_deref());
    line.push('\n');
    line
}

pub fn entry_line(index: u32) -> String {
    let memo = entry_memo(index);
    let amount = entry_amount_minor(index).map(format_minor);
    let mut line = String::new();
    line.push_str(&index.to_string());
    line.push('\t');
    line.push_str(&entry_account(index).to_string());
    line.push('\t');
    line.push_str(&entry_qty(index).to_string());
    line.push('\t');
    copy_field(&mut line, amount.as_deref());
    line.push('\t');
    copy_field(&mut line, memo.as_deref());
    line.push('\n');
    line
}

pub struct KitPin {
    pub role: &'static str,
    pub relative: &'static str,
    pub sha256: &'static str,
}

pub fn kit_pins() -> &'static [KitPin] {
    &[
        KitPin {
            role: "pi_executable",
            relative: "install/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js",
            sha256: "e79626f2dd6f94aa45d30f3fa63cd84319a6eefcd150b353cfaf274366926774",
        },
        KitPin {
            role: "pi_runtime",
            relative: "node/bin/node",
            sha256: "d09e299258c24f7cdf6f5d5ec185e3a56512b27a697113735dac909f1cac7b8d",
        },
        KitPin {
            role: "oo_bin",
            relative: "bin/oo",
            sha256: "2fd7a6209bd965aca5b489d41e793a612bb2d3015b5a611e73a350a05d814898",
        },
        KitPin {
            role: "query_pg_bin",
            relative: "bin/query-pg",
            sha256: "3cf46f6dc6b168b982edf09b06c88794e471dcc27b3f2471de1490a4ae2027ab",
        },
        KitPin {
            role: "bash",
            relative: "node/bin/bash",
            sha256: "ccbd5106945d1095474e5ee47317266f5918aa37fc671829192a007918b1413a",
        },
        KitPin {
            role: "rg",
            relative: "node/bin/rg",
            sha256: "e07d5c85fa9ca740ff4ab8bbac60a1e11c7a5ce242435f7820a03f7c20ef6276",
        },
        KitPin {
            role: "static_library",
            relative: "lib/libpg_query.a",
            sha256: "73271b02b7135e30515f4ac566f886f64053422830fc8c5b98e95c00b66ec193",
        },
        KitPin {
            role: "cjk_font",
            relative: "fonts/NotoSansCJKsc-Regular.otf",
            sha256: "2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b",
        },
    ]
}

pub fn answer_token(id: &str) -> Option<&'static str> {
    match id {
        "count-batch-c" => Some(BULK_COUNT_C),
        "id-overlap" => Some(BULK_ID_OVERLAP),
        "amount-when-qty-above" => Some(BULK_AMOUNT_SUM),
        "null-amount" => Some(BULK_NULL_AMOUNT),
        "batch-a-amount" => Some(BULK_AMOUNT_EXACT),
        "batch-c-qty-bounds" => Some(BULK_QTY_MIN),
        "count-entry" => Some("90000"),
        "fk-account" | "null-party" => Some("p00000"),
        "qty-bounds" => Some("-9223372036854775808"),
        "amount-exact" => Some("1234567890.1234"),
        "null-display-count" => Some("118"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        answer_token, expected_cells, format_minor, frozen_questions, null_display_count, prompt_leaks, shop_questions,
        ACCOUNT_ROWS, BULK_AMOUNT_EXACT, BULK_AMOUNT_SUM, BULK_COUNT_C, BULK_ID_OVERLAP, BULK_NULL_AMOUNT, BULK_QTY_MAX,
        BULK_QTY_MIN, ENTRY_ROWS, PARTY_ROWS,
    };

    #[test]
    fn public_gold_matches_the_frozen_questions() {
        assert_eq!((PARTY_ROWS, ACCOUNT_ROWS, ENTRY_ROWS), (2000, 8000, 90000));
        assert_eq!(null_display_count(), 118);
        assert_eq!(format_minor(12_345_678_901_234), "1234567890.1234");
        assert_eq!(expected_cells("count-entry").expect("count")[0][0], super::Cell::Text("90000".to_owned()));
        assert_eq!(answer_token("null-display-count"), Some("118"));
        let shop_qty = expected_cells("qty-bounds").expect("qty");
        assert_eq!(shop_qty[0][1], super::Cell::Text(i64::MIN.to_string()));
        assert_eq!(shop_qty[1][1], super::Cell::Text(i64::MAX.to_string()));
        assert_eq!(shop_qty[2][1], super::Cell::Text("0".to_owned()));
        assert_eq!(expected_cells("amount-exact").expect("shop amount")[0][0], super::Cell::Text("1234567890.1234".to_owned()));
        assert_eq!(expected_cells("count-batch-c").expect("count")[0][0], super::Cell::Text(BULK_COUNT_C.to_owned()));
        assert_eq!(expected_cells("id-overlap").expect("overlap")[0][0], super::Cell::Text(BULK_ID_OVERLAP.to_owned()));
        assert_eq!(expected_cells("amount-when-qty-above").expect("sum")[0][0], super::Cell::Text(BULK_AMOUNT_SUM.to_owned()));
        assert_eq!(expected_cells("null-amount").expect("null")[0][0], super::Cell::Text(BULK_NULL_AMOUNT.to_owned()));
        assert_eq!(expected_cells("batch-a-amount").expect("amount")[0][0], super::Cell::Text(BULK_AMOUNT_EXACT.to_owned()));
        let bounds = expected_cells("batch-c-qty-bounds").expect("bounds");
        assert_eq!(bounds[0][1], super::Cell::Text(BULK_QTY_MAX.to_owned()));
        assert_eq!(bounds[1][1], super::Cell::Text(BULK_QTY_MIN.to_owned()));
        let party = &expected_cells("null-party").expect("party")[0];
        assert_eq!(party[1], super::Cell::Null);
        assert_eq!(party[2], super::Cell::Bool(true));
        assert_eq!(frozen_questions().len(), 6);
        assert_eq!(shop_questions().len(), 6);
        for question in shop_questions() {
            assert!(!prompt_leaks(question.text), "{}", question.id);
            assert!(expected_cells(question.id).is_some(), "{}", question.id);
        }
        for question in frozen_questions() {
            let token = answer_token(question.id).expect(question.id);
            assert!(!prompt_leaks(question.text), "{}", question.id);
            assert!(!question.text.contains(token), "{}", question.id);
            assert!(expected_cells(question.id).is_some(), "{}", question.id);
        }
    }

    #[test]
    fn copy_lines_cover_null_bounds_and_decimal() {
        let party = super::party_line(0);
        assert!(party.starts_with("p00000\t\\N\tt\t"), "{party}");
        assert!(super::entry_line(0).contains("-9223372036854775808"));
        assert!(super::entry_line(1).contains("9223372036854775807"));
        assert!(super::entry_line(3).contains("1234567890.1234"));
        assert!(super::schema_sql().contains("numeric(18,4)"));
        assert!(!super::schema_sql().contains("oo_a317"));
        assert!(!super::false_success("rejected", Some(1), Some(400)));
        assert!(super::false_success("validated", Some(0), None));
        assert_eq!(super::kit_pins().len(), 8);
        assert_eq!(super::kit_pins()[3].role, "query_pg_bin");
        assert_eq!(super::kit_pins()[3].sha256.len(), 64);
    }
}
