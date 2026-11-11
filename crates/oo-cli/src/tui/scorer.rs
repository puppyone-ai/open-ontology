//! Table recommendation for screen 2. Rules plus catalog metadata only: no
//! data rows, no model, and table names never leave this machine.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Catalog-only facts about one table.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TableMeta {
    pub schema: String,
    pub name: String,
    /// Row estimate from statistics; `None` when the engine did not report one.
    pub rows: Option<u64>,
    pub has_primary_key: bool,
    pub has_unique_key: bool,
    pub partition_child: bool,
    /// Qualified `schema.table` targets of outgoing foreign keys.
    pub references: Vec<String>,
    pub columns: BTreeMap<String, String>,
}

impl TableMeta {
    pub fn qualified(&self) -> String {
        format!("{}.{}", self.schema, self.name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Recommend,
    Maybe,
    Skip,
}

impl Verdict {
    pub fn glyph(self) -> &'static str {
        match self {
            Verdict::Recommend => "✓",
            Verdict::Maybe => "?",
            Verdict::Skip => "✗",
        }
    }
}

/// Why a table got its verdict. `code` is stable for `--json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum Reason {
    NameLooksTechnical { pattern: String },
    PartitionChild,
    Empty,
    NoKey,
    CodeTable { referenced_by: usize },
    Hub { referenced_by: usize },
    Referenced { referenced_by: usize },
    LinksEntities { references: usize },
    BusinessName { word: String },
    ProbableLinks { referenced_by: usize },
    LookupName,
    RankedFallback,
    NoSignal,
}

impl Reason {
    pub fn text(&self, zh: bool) -> String {
        match (self, zh) {
            (Reason::NameLooksTechnical { pattern }, false) => {
                format!("name looks technical ({pattern})")
            }
            (Reason::NameLooksTechnical { pattern }, true) => {
                format!("名称疑似日志/临时/框架表（{pattern}）")
            }
            (Reason::PartitionChild, false) => "partition child; the parent is listed".into(),
            (Reason::PartitionChild, true) => "分区子表，选父表即可".into(),
            (Reason::Empty, false) => "empty table".into(),
            (Reason::Empty, true) => "空表".into(),
            (Reason::NoKey, false) => "no primary or unique key".into(),
            (Reason::NoKey, true) => "没有主键也没有唯一键".into(),
            (Reason::CodeTable { referenced_by }, false) => {
                format!("code table: small, referenced by {referenced_by}")
            }
            (Reason::CodeTable { referenced_by }, true) => {
                format!("码表：小表，被 {referenced_by} 张表引用")
            }
            (Reason::Hub { referenced_by }, false) => {
                format!("hub: referenced by {referenced_by} tables")
            }
            (Reason::Hub { referenced_by }, true) => {
                format!("实体中心：被 {referenced_by} 张表引用")
            }
            (Reason::Referenced { referenced_by }, false) => {
                format!("referenced by {referenced_by} table")
            }
            (Reason::Referenced { referenced_by }, true) => format!("被 {referenced_by} 张表引用"),
            (Reason::LinksEntities { references }, false) => format!("links {references} tables"),
            (Reason::LinksEntities { references }, true) => format!("关联 {references} 张表"),
            (Reason::BusinessName { word }, false) => format!("business-like name ({word})"),
            (Reason::BusinessName { word }, true) => format!("名称像业务对象（{word}）"),
            (Reason::ProbableLinks { referenced_by }, false) => {
                format!("no declared keys; ranked by shared key columns ({referenced_by} probable links)")
            }
            (Reason::ProbableLinks { referenced_by }, true) => {
                format!("无声明关联键；按共享键列排序（{referenced_by} 个可能关联）")
            }
            (Reason::LookupName, false) => "small code/lookup table by name".into(),
            (Reason::LookupName, true) => "名称疑似小型码表".into(),
            (Reason::RankedFallback, false) => {
                "no declared keys; ranked by shared key columns, lookup names and size tiers (fallback)".into()
            }
            (Reason::RankedFallback, true) => "无声明关联键；按共享键列、码表名称和规模排序（兜底推荐）".into(),
            (Reason::NoSignal, false) => "no strong signal either way".into(),
            (Reason::NoSignal, true) => "没有明显信号".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scored {
    pub table: String,
    pub verdict: Verdict,
    pub reason: Reason,
    pub referenced_by: usize,
    pub rows: Option<u64>,
}

/// A table this small and referenced is treated as a code table.
pub const CODE_TABLE_MAX_ROWS: u64 = 200;

const SKIP_SUFFIX: [&str; 5] = ["_log", "_tmp", "_bak", "_old", "_history"];
const SKIP_PREFIX: [&str; 5] = ["audit", "queue", "session", "flyway_", "django_"];
const SKIP_EXACT: [&str; 1] = ["schema_migrations"];
const BUSINESS: [&str; 34] = [
    "order",
    "customer",
    "client",
    "product",
    "item",
    "invoice",
    "payment",
    "account",
    "shipment",
    "shipping",
    "parcel",
    "package",
    "delivery",
    "route",
    "vehicle",
    "driver",
    "carrier",
    "warehouse",
    "stock",
    "inventory",
    "supplier",
    "vendor",
    "employee",
    "staff",
    "store",
    "branch",
    "department",
    "contract",
    "region",
    "city",
    "station",
    "trip",
    "订单",
    "客户",
];

fn technical(name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    if let Some(s) = SKIP_SUFFIX.iter().find(|s| lower.ends_with(*s)) {
        return Some(format!("*{s}"));
    }
    if let Some(p) = SKIP_PREFIX.iter().find(|p| lower.starts_with(*p)) {
        return Some(format!("{p}*"));
    }
    SKIP_EXACT
        .iter()
        .find(|e| lower == **e)
        .map(|e| (*e).to_owned())
}

fn business(name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    BUSINESS
        .iter()
        .find(|w| lower.contains(*w))
        .map(|w| (*w).to_owned())
}

fn eligible(table: &TableMeta) -> bool {
    technical(&table.name).is_none()
        && !table.partition_child
        && table.rows != Some(0)
        && (table.has_primary_key || table.has_unique_key)
}

fn lookup(name: &str) -> bool {
    name.to_lowercase().split('_').any(|part| {
        matches!(
            part,
            "code" | "codes" | "lookup" | "dict" | "dictionary" | "ref" | "cd"
        )
    })
}

fn type_family(native: &str) -> Option<&'static str> {
    let lower = native.to_ascii_lowercase();
    let base = lower.split(['(', ' ']).next().unwrap_or("");
    match base {
        "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "bigint" | "int2" | "int4"
        | "int8" => Some("integer"),
        "char" | "varchar" | "varchar2" | "nvarchar" | "nvarchar2" | "text" | "bpchar"
        | "nchar" => Some("text"),
        "decimal" | "numeric" | "number" => Some("decimal"),
        "uuid" => Some("uuid"),
        _ => None,
    }
}

fn size_tier(rows: Option<u64>) -> u8 {
    match rows {
        Some(1..=200) => 1,
        Some(201..=10_000) => 2,
        Some(10_001..) => 3,
        _ => 0,
    }
}

/// Score every table. Output order: recommended, maybe, skipped; then by name.
pub fn score(tables: &[TableMeta]) -> Vec<Scored> {
    // Generic "id", timestamps and amounts are not evidence of a link.
    let mut keys: BTreeMap<(String, &str), BTreeSet<usize>> = BTreeMap::new();
    for (index, table) in tables.iter().enumerate().filter(|(_, t)| eligible(t)) {
        for (name, native) in &table.columns {
            let name = name.to_lowercase();
            if name.len() > 2
                && ["id", "_cd", "_no"]
                    .iter()
                    .any(|suffix| name.ends_with(suffix))
                && let Some(family) = type_family(native)
            {
                keys.entry((name, family)).or_default().insert(index);
            }
        }
    }
    let mut out: Vec<Scored> = tables
        .iter()
        .enumerate()
        .map(|(index, table)| {
            let me = table.qualified();
            let declared = tables
                .iter()
                .filter(|other| other.qualified() != me && other.references.contains(&me))
                .count();
            let mut linked = BTreeSet::new();
            if eligible(table) {
                for (name, native) in &table.columns {
                    if let Some(family) = type_family(native)
                        && let Some(peers) = keys.get(&(name.to_lowercase(), family))
                    {
                        linked.extend(peers.iter().copied().filter(|peer| *peer != index));
                    }
                }
            }
            let referenced_by = tables
                .iter()
                .enumerate()
                .filter(|(i, other)| {
                    *i != index && (other.references.contains(&me) || linked.contains(i))
                })
                .count();
            let outgoing = {
                let mut targets = table.references.clone();
                targets.sort();
                targets.dedup();
                targets.retain(|t| t != &me);
                targets.len()
            };
            let (verdict, reason) = if let Some(pattern) = technical(&table.name) {
                (Verdict::Skip, Reason::NameLooksTechnical { pattern })
            } else if table.partition_child {
                (Verdict::Skip, Reason::PartitionChild)
            } else if table.rows == Some(0) {
                (Verdict::Skip, Reason::Empty)
            } else if !table.has_primary_key && !table.has_unique_key {
                (Verdict::Skip, Reason::NoKey)
            } else if declared == 0 && !linked.is_empty() {
                (Verdict::Recommend, Reason::ProbableLinks { referenced_by })
            } else if referenced_by > 0 && table.rows.is_some_and(|r| r <= CODE_TABLE_MAX_ROWS) {
                (Verdict::Recommend, Reason::CodeTable { referenced_by })
            } else if referenced_by >= 2 {
                (Verdict::Recommend, Reason::Hub { referenced_by })
            } else if referenced_by == 1 {
                (Verdict::Recommend, Reason::Referenced { referenced_by })
            } else if outgoing >= 2 {
                (
                    Verdict::Recommend,
                    Reason::LinksEntities {
                        references: outgoing,
                    },
                )
            } else if let Some(word) = business(&table.name) {
                (Verdict::Recommend, Reason::BusinessName { word })
            } else if lookup(&table.name) && table.rows.is_some_and(|r| r <= CODE_TABLE_MAX_ROWS) {
                (Verdict::Recommend, Reason::LookupName)
            } else {
                (Verdict::Maybe, Reason::NoSignal)
            };
            Scored {
                table: me,
                verdict,
                reason,
                referenced_by,
                rows: table.rows,
            }
        })
        .collect();
    if !out.iter().any(|s| s.verdict == Verdict::Recommend) {
        let mut candidates: Vec<_> = out
            .iter_mut()
            .filter(|s| s.verdict == Verdict::Maybe)
            .collect();
        candidates.sort_by(|a, b| {
            b.referenced_by
                .cmp(&a.referenced_by)
                .then(lookup(&b.table).cmp(&lookup(&a.table)))
                .then(size_tier(b.rows).cmp(&size_tier(a.rows)))
                .then(a.table.cmp(&b.table))
        });
        for candidate in candidates.into_iter().take(5) {
            candidate.verdict = Verdict::Recommend;
            candidate.reason = Reason::RankedFallback;
        }
    }
    out.sort_by(|a, b| {
        let rank = |v: Verdict| match v {
            Verdict::Recommend => 0,
            Verdict::Maybe => 1,
            Verdict::Skip => 2,
        };
        rank(a.verdict)
            .cmp(&rank(b.verdict))
            .then(b.referenced_by.cmp(&a.referenced_by))
            .then(a.table.cmp(&b.table))
    });
    out
}

/// The explicit list saved for `--tables-recommended`.
pub fn recommended(scored: &[Scored]) -> Vec<String> {
    let mut list: Vec<String> = scored
        .iter()
        .filter(|s| s.verdict == Verdict::Recommend)
        .map(|s| s.table.clone())
        .collect();
    list.sort();
    list
}

/// Read a catalog file: either `oo.table-catalog/v1` (`{"tables":[TableMeta]}`)
/// or an `oo.source.pgsql/v1` source catalog (no row estimates).
pub fn from_catalog(value: &Value) -> Result<Vec<TableMeta>, &'static str> {
    let tables = value["tables"].as_array().ok_or("TABLE_CATALOG_SHAPE")?;
    if value["protocol"] == "oo.table-catalog/v1" {
        return tables
            .iter()
            .map(|t| serde_json::from_value(t.clone()).map_err(|_| "TABLE_CATALOG_SHAPE"))
            .collect();
    }
    let mut out = Vec::new();
    for t in tables {
        let schema = t["schema"].as_str().ok_or("TABLE_CATALOG_SHAPE")?;
        let name = t["name"].as_str().ok_or("TABLE_CATALOG_SHAPE")?;
        let mut references = Vec::new();
        for fk in t["foreign_keys"].as_array().into_iter().flatten() {
            if let Some(col) = fk["columns"].as_array().and_then(|c| c.first()) {
                references.push(format!(
                    "{}.{}",
                    col["ref_schema"].as_str().unwrap_or(schema),
                    col["ref_table"].as_str().unwrap_or_default()
                ));
            }
        }
        out.push(TableMeta {
            schema: schema.into(),
            name: name.into(),
            rows: t["rows"].as_u64(),
            has_primary_key: t["primary_key"].as_array().is_some_and(|k| !k.is_empty()),
            has_unique_key: t["unique_keys"].as_array().is_some_and(|k| !k.is_empty()),
            partition_child: t["relispartition"] == true,
            references,
            columns: t["columns"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| {
                    Some((
                        c["name"].as_str()?.to_owned(),
                        c["data_type"]
                            .as_str()
                            .or_else(|| c["type"].as_str())?
                            .to_owned(),
                    ))
                })
                .collect(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(name: &str, rows: Option<u64>, pk: bool, refs: &[&str]) -> TableMeta {
        TableMeta {
            schema: "s".into(),
            name: name.into(),
            rows,
            has_primary_key: pk,
            has_unique_key: false,
            partition_child: false,
            references: refs.iter().map(|r| format!("s.{r}")).collect(),
            columns: BTreeMap::new(),
        }
    }

    fn verdict(scored: &[Scored], name: &str) -> (Verdict, Reason) {
        let s = scored
            .iter()
            .find(|s| s.table == format!("s.{name}"))
            .unwrap();
        (s.verdict, s.reason.clone())
    }

    #[test]
    fn legacy_shared_keys_are_probable_not_declared_links() {
        let mut tables = vec![
            t("kh", Some(200), true, &[]),
            t("dd", Some(2000), true, &[]),
            t("x_log", Some(2000), true, &[]),
            t("zz", Some(200), true, &[]),
        ];
        for table in &mut tables {
            table.columns.insert("khid".into(), "bigint".into());
        }
        tables[3]
            .columns
            .insert("khid".into(), "varchar(20)".into());
        let scored = score(&tables);
        assert_eq!(recommended(&scored), ["s.dd", "s.kh"]);
        assert_eq!(
            verdict(&scored, "kh").1,
            Reason::ProbableLinks { referenced_by: 1 }
        );
        assert!(
            verdict(&scored, "kh")
                .1
                .text(false)
                .contains("ranked by shared key columns")
        );
        assert!(tables.iter().all(|t| t.references.is_empty()));
    }

    #[test]
    fn fallback_is_nonempty_deterministic_bounded_and_never_promotes_skips() {
        let mut tables: Vec<_> = (0..7)
            .map(|i| t(&format!("x{i}"), Some(500), true, &[]))
            .collect();
        tables.extend([
            t("x_log", Some(100_000), true, &[]),
            t("x_tmp", Some(100_000), true, &[]),
            t("x_empty", Some(0), true, &[]),
            t("x_keyless", Some(100_000), false, &[]),
        ]);
        let first = score(&tables);
        tables.reverse();
        assert_eq!(score(&tables), first);
        assert_eq!(
            recommended(&first),
            ["s.x0", "s.x1", "s.x2", "s.x3", "s.x4"]
        );
        assert_eq!(verdict(&first, "x0").1, Reason::RankedFallback);
        assert!(recommended(&score(&[t("x_log", Some(1), true, &[])])).is_empty());
    }

    #[test]
    fn generic_columns_do_not_link_and_small_lookup_names_are_recommended() {
        let mut tables = vec![
            t("status_cd", Some(10), true, &[]),
            t("x", Some(500), true, &[]),
        ];
        for table in &mut tables {
            table.columns.insert("id".into(), "int".into());
            table.columns.insert("amount".into(), "decimal".into());
        }
        let scored = score(&tables);
        assert_eq!(verdict(&scored, "status_cd").1, Reason::LookupName);
        assert_eq!(verdict(&scored, "x").0, Verdict::Maybe);
        assert!(scored.iter().all(|s| s.referenced_by == 0));
    }

    #[test]
    fn skips_technical_names_empty_partitions_and_keyless_tables() {
        let mut tables = vec![
            t("shipment_log", Some(10), true, &[]),
            t("import_tmp", Some(10), true, &[]),
            t("orders_bak", Some(10), true, &[]),
            t("price_old", Some(10), true, &[]),
            t("status_history", Some(10), true, &[]),
            t("audit_events", Some(10), true, &[]),
            t("queue_jobs", Some(10), true, &[]),
            t("sessions", Some(10), true, &[]),
            t("flyway_schema_history", Some(10), true, &[]),
            t("django_migrations", Some(10), true, &[]),
            t("schema_migrations", Some(10), true, &[]),
            t("empty_orders", Some(0), true, &[]),
            t("raw_feed", Some(10), false, &[]),
            t("orders_2024_01", Some(10), true, &[]),
        ];
        tables.last_mut().unwrap().partition_child = true;
        let scored = score(&tables);
        assert!(
            scored.iter().all(|s| s.verdict == Verdict::Skip),
            "{scored:?}"
        );
        assert_eq!(verdict(&scored, "empty_orders").1, Reason::Empty);
        assert_eq!(verdict(&scored, "raw_feed").1, Reason::NoKey);
        assert_eq!(verdict(&scored, "orders_2024_01").1, Reason::PartitionChild);
        assert_eq!(
            verdict(&scored, "Shipment_LOG".to_lowercase().as_str()).1,
            Reason::NameLooksTechnical {
                pattern: "*_log".into()
            }
        );
    }

    #[test]
    fn recommends_hubs_code_tables_links_and_business_names() {
        let tables = vec![
            t("customers", Some(120_000), true, &[]),
            t(
                "orders",
                Some(900_000),
                true,
                &["customers", "status_codes"],
            ),
            t("invoices", Some(50_000), true, &["customers", "orders"]),
            t("status_codes", Some(12), true, &[]),
            t("x_link", Some(900), true, &["orders", "invoices"]),
            t("warehouse", Some(40), true, &[]),
            t("misc_stuff", Some(40), true, &[]),
        ];
        let scored = score(&tables);
        assert_eq!(
            verdict(&scored, "customers"),
            (Verdict::Recommend, Reason::Hub { referenced_by: 2 })
        );
        assert_eq!(
            verdict(&scored, "status_codes"),
            (Verdict::Recommend, Reason::CodeTable { referenced_by: 1 })
        );
        assert_eq!(verdict(&scored, "invoices").0, Verdict::Recommend);
        assert_eq!(
            verdict(&scored, "x_link").1,
            Reason::LinksEntities { references: 2 }
        );
        assert_eq!(
            verdict(&scored, "warehouse").1,
            Reason::BusinessName {
                word: "warehouse".into()
            }
        );
        assert_eq!(
            verdict(&scored, "misc_stuff"),
            (Verdict::Maybe, Reason::NoSignal)
        );
        assert_eq!(scored[0].table, "s.customers", "hubs sort first");
        assert!(!recommended(&scored).contains(&"s.misc_stuff".to_owned()));
        assert_eq!(
            Reason::Hub { referenced_by: 6 }.text(true),
            "实体中心：被 6 张表引用"
        );
    }

    #[test]
    fn unknown_row_counts_never_make_a_code_table_and_self_references_do_not_count() {
        let tables = vec![
            t("employees", None, true, &["employees"]),
            t("orders", None, true, &["employees"]),
        ];
        let scored = score(&tables);
        assert_eq!(
            verdict(&scored, "employees").1,
            Reason::Referenced { referenced_by: 1 }
        );
    }

    #[test]
    fn reads_source_catalog_shape() {
        let value = serde_json::json!({"protocol":"oo.source.pgsql/v1","tables":[
            {"schema":"s","name":"orders","primary_key":[{"attnum":1,"name":"id"}],"unique_keys":[],
             "foreign_keys":[{"name":"fk","columns":[{"attnum":2,"name":"customer_id","ref_schema":"s","ref_table":"customers","ref_attnum":1,"ref_name":"id"}]}]}]});
        let tables = from_catalog(&value).unwrap();
        assert_eq!(tables[0].references, vec!["s.customers"]);
        assert!(tables[0].has_primary_key);
        assert_eq!(tables[0].rows, None);
    }
}
