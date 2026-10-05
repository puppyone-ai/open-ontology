//! Enterprise fixture for this integration only. Two tables, one foreign key.
//! Types stay inside the executor's bool / int8 / text set.

pub const READER: &str = "oo_a203_reader";
pub const RUNTIME: &str = "oo_a203_runtime";
pub const LOGIN_PARTY: &str = "oo_a203_login_party";
pub const LOGIN_INVOICE: &str = "oo_a203_login_invoice";
pub const ROLE_PARTY: &str = "oo_r_party_read";
pub const ROLE_INVOICE: &str = "oo_r_invoice_read";
pub const SLUG_PARTY: &str = "party_read";
pub const SLUG_INVOICE: &str = "invoice_read";
pub const SOURCE_DB: &str = "oo_a203_src";
pub const TARGET_DB: &str = "oo_a203_dst";
pub const SOURCE_ID: &str = "a203ent";
pub const SHARD: &str = "a203main";
pub const OWNER: &str = "eef831aa-dba4-4af2-a78b-541bc9e0a203";
pub const SECRET_REF: &str = "secret://a203/source-reader";
pub const HIDDEN_PARTY: &str = "internal_note";
pub const HIDDEN_INVOICE: &str = "clerk_note";

pub const SCHEMA_SQL: &[&str] = &[
    "CREATE SCHEMA ent",
    "CREATE TABLE ent.party (
        party_id int8 PRIMARY KEY,
        display_name text,
        active bool NOT NULL,
        credit_limit int8 NOT NULL,
        internal_note text
    )",
    "COMMENT ON TABLE ent.party IS '交易方主数据'",
    "COMMENT ON COLUMN ent.party.display_name IS '可空中文名称'",
    "COMMENT ON COLUMN ent.party.internal_note IS '不对主体角色公开'",
    "CREATE TABLE ent.invoice (
        invoice_id int8 PRIMARY KEY,
        party_id int8 NOT NULL,
        memo text,
        posted bool NOT NULL,
        amount int8,
        clerk_note text,
        CONSTRAINT invoice_party_fk FOREIGN KEY (party_id) REFERENCES ent.party (party_id)
    )",
    "COMMENT ON TABLE ent.invoice IS '发票单据'",
    "COMMENT ON COLUMN ent.invoice.memo IS '可空中文摘要'",
    "COMMENT ON COLUMN ent.invoice.clerk_note IS '不对发票角色公开'",
    "INSERT INTO ent.party (party_id, display_name, active, credit_limit, internal_note) VALUES
        (1, '华能机电', true, 9223372036854775807, '限额'),
        (2, NULL, false, -9223372036854775808, NULL),
        (3, '李四', true, 0, '空额')",
    "INSERT INTO ent.invoice (invoice_id, party_id, memo, posted, amount, clerk_note) VALUES
        (100, 1, '首张发票', true, 9223372036854775807, '已核'),
        (101, 1, NULL, false, NULL, NULL),
        (102, 2, '负数边界', true, -9223372036854775808, '边界')",
    "GRANT USAGE ON SCHEMA ent TO oo_a203_reader",
    "GRANT SELECT ON ent.party, ent.invoice TO oo_a203_reader",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    Null,
    Bool(bool),
    Int(&'static str),
    Text(&'static str),
}

pub const PARTY_ALL: &[&str] = &[
    "party_id",
    "display_name",
    "active",
    "credit_limit",
    "internal_note",
];

pub const PARTY_VISIBLE: &[&str] = &["party_id", "display_name", "active", "credit_limit"];

pub const INVOICE_ALL: &[&str] = &[
    "invoice_id",
    "party_id",
    "memo",
    "posted",
    "amount",
    "clerk_note",
];

pub const INVOICE_VISIBLE: &[&str] = &["invoice_id", "party_id", "memo", "posted", "amount"];

pub const PARTY_ROWS: &[&[Expect]] = &[
    &[
        Expect::Int("1"),
        Expect::Text("华能机电"),
        Expect::Bool(true),
        Expect::Int("9223372036854775807"),
        Expect::Text("限额"),
    ],
    &[
        Expect::Int("2"),
        Expect::Null,
        Expect::Bool(false),
        Expect::Int("-9223372036854775808"),
        Expect::Null,
    ],
    &[
        Expect::Int("3"),
        Expect::Text("李四"),
        Expect::Bool(true),
        Expect::Int("0"),
        Expect::Text("空额"),
    ],
];

pub const INVOICE_ROWS: &[&[Expect]] = &[
    &[
        Expect::Int("100"),
        Expect::Int("1"),
        Expect::Text("首张发票"),
        Expect::Bool(true),
        Expect::Int("9223372036854775807"),
        Expect::Text("已核"),
    ],
    &[
        Expect::Int("101"),
        Expect::Int("1"),
        Expect::Null,
        Expect::Bool(false),
        Expect::Null,
        Expect::Null,
    ],
    &[
        Expect::Int("102"),
        Expect::Int("2"),
        Expect::Text("负数边界"),
        Expect::Bool(true),
        Expect::Int("-9223372036854775808"),
        Expect::Text("边界"),
    ],
];

pub fn semantic_type(type_name: &str) -> Option<&'static str> {
    match type_name {
        "int8" => Some("int8"),
        "bool" => Some("bool"),
        "text" => Some("text"),
        _ => None,
    }
}
