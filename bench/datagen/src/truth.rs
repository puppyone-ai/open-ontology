//! Hidden semantic truth. These documents are benchmark material. They are
//! not source comments and they are not a product Pack.

use crate::facts::{
    AS_OF, AUTHZ_POLICY, BUSINESS_TZ, CLOSING_LAG_DAYS, DATASET, EPOCH, FISCAL_START_MONTH,
    HX_INVOICE_DATE, HX_INVOICE_FEN, HX_ORDER_FEN, HX_RECEIPT_APRIL_DATE, HX_RECEIPT_APRIL_FEN,
    HX_RECEIPT_MARCH_DATE, MISMATCH_REF, ORDER_REF, TRUTH_MARKER,
};
use crate::jsonutil::{J, arr, b, obj, s, u};
use crate::plan::{Counts, SmellSet};
use crate::prng::PRNG_VERSION;
use crate::rows::Side;

pub struct TruthIn<'a> {
    pub scale: &'a str,
    pub seed: u64,
    pub part: u16,
    pub counts: &'a Counts,
    pub smells: &'a SmellSet,
    pub side: &'a Side,
    pub shards: &'a [(u32, u64)],
    pub tzdb: &'a str,
}

pub fn readme() -> String {
    format!(
        "OO-Logistics hidden truth for the public benchmark.\n\
{TRUTH_MARKER}\n\
entity_cluster records and do_not_merge decisions live in this directory.\n\
expected_decision values live under authorization and attacks.\n\
gold_sql is not produced in SP00-W04.\n\
This directory must not be copied into a product Pack.\n\
Source comments are untrusted and are not applied.\n"
    )
}

pub fn semantics(smells: &SmellSet) -> J {
    let orders = if smells.enabled("cents") {
        "Sales orders. Amounts are integer fen."
    } else {
        "Sales orders. Amounts are exact decimal yuan at scale 2."
    };
    let receipts = if smells.enabled("closing_period") {
        "Receipts. jzbz marks a closing adjustment."
    } else {
        "Receipts. jzbz is 0 because closing_period is off."
    };
    let rows = [
        (
            "kh",
            "客户",
            "mysql-erp",
            "Customer master. Legacy name kh.",
        ),
        (
            "kh_bm",
            "别名",
            "mysql-erp",
            "Customer aliases. Composite key with the customer.",
        ),
        (
            "ht",
            "合同",
            "mysql-erp",
            "Contracts. Composite key of id and revision.",
        ),
        (
            "dd",
            "订单",
            "mysql-erp",
            orders,
        ),
        ("ys", "应收", "mysql-erp", "Receivables, one per order."),
        (
            "sk",
            "收款",
            "mysql-erp",
            receipts,
        ),
        ("tk", "退款", "mysql-erp", "Refunds against a receipt."),
        ("fp", "发票", "mysql-erp", "Invoices."),
        (
            "zd",
            "字典",
            "mysql-erp",
            "Code dictionary. Meanings can change by date.",
        ),
        (
            "yh",
            "用户",
            "mysql-erp",
            "Application users and role codes.",
        ),
        (
            "yd",
            "运单",
            "postgres-tms",
            "Waybills. Customer column is kh_dm, not khid.",
        ),
        ("yd_mx", "明细", "postgres-tms", "Waybill lines."),
        (
            "gj",
            "轨迹",
            "postgres-tms",
            "Tracking events. Monthly shards. No primary key.",
        ),
        ("cl", "车辆", "postgres-tms", "Vehicles."),
        (
            "sj",
            "司机",
            "postgres-tms",
            "Drivers. Identity number is a canary column.",
        ),
        ("xl", "线路", "postgres-tms", "Routes."),
        (
            "ck",
            "仓库",
            "postgres-tms",
            "Warehouses. Region column is dq_dm.",
        ),
        (
            "kc",
            "库存",
            "postgres-tms",
            "Inventory by warehouse, sku, and month.",
        ),
    ];
    let items = rows
        .into_iter()
        .map(|(table, zh, source, text)| {
            obj(vec![
                ("table", s(table)),
                ("business_name", s(zh)),
                ("source", s(source)),
                ("meaning", s(text)),
            ])
        })
        .collect();
    obj(vec![("dataset", s(DATASET)), ("tables", arr(items))])
}

pub fn relationships(input: &TruthIn<'_>) -> J {
    let scenario = input.side.scenario.as_ref();
    obj(vec![
        ("foreign_keys", u(0)),
        (
            "joins",
            arr(vec![
                obj(vec![
                    ("left", s("kh.khid")),
                    ("right", s("yd.kh_dm")),
                    ("kind", s("same_customer")),
                ]),
                obj(vec![
                    ("left", s("dd.ddid")),
                    ("right", s("yd.erp_ddh")),
                    ("kind", s("order_waybill")),
                    ("except_token", s("NO-SUCH-ORDER")),
                ]),
                obj(vec![
                    ("left", s("dd.je_fen")),
                    ("right", s("yd.sfje")),
                    ("kind", s("header_amount")),
                ]),
                obj(vec![
                    ("left", s("sum(yd_mx.je_fen)")),
                    ("right", s("yd.sfje")),
                    ("kind", s("line_sum")),
                    ("except_ref", s(MISMATCH_REF)),
                ]),
            ]),
        ),
        (
            "dangling_waybill",
            s(scenario
                .map(|item| item.dangling_ydh.as_str())
                .unwrap_or("")),
        ),
        ("mismatch_ref", s(MISMATCH_REF)),
    ])
}

pub fn enums_doc(input: &TruthIn<'_>) -> J {
    obj(vec![
        (
            "khzt",
            arr(vec![
                obj(vec![("code", s("0")), ("label", s("停用"))]),
                obj(vec![("code", s("1")), ("label", s("正常"))]),
                obj(vec![
                    ("code", s("2")),
                    ("from", s("2000-01-01")),
                    ("to", s("2023-12-31")),
                    ("label", s("冻结")),
                    ("enabled", b(input.smells.enabled("code_meaning_by_time"))),
                ]),
                obj(vec![
                    ("code", s("2")),
                    ("from", s("2024-01-01")),
                    ("to", s("9999-12-31")),
                    ("label", s("停用待复核")),
                ]),
            ]),
        ),
        (
            "gaps",
            arr(vec![
                obj(vec![
                    ("code", s("khfl=7")),
                    ("in_dictionary", b(false)),
                    ("enabled", b(input.smells.enabled("dictionary_gap"))),
                ]),
                obj(vec![
                    ("code", s("ydzt=99")),
                    ("in_dictionary", b(false)),
                    ("enabled", b(input.smells.enabled("dictionary_gap"))),
                ]),
            ]),
        ),
        (
            "note",
            s(
                "zt=2 before 2024-01-01 means 冻结. The same code on or after 2024-01-01 means 停用待复核.",
            ),
        ),
    ])
}

pub fn entities(input: &TruthIn<'_>) -> J {
    let scenario = input.side.scenario.as_ref();
    let blank = "";
    let hx = scenario.map(|item| item.hx_khid.as_str()).unwrap_or(blank);
    let unit = scenario.map(|item| item.hx_unit.as_str()).unwrap_or(blank);
    let sim_a = scenario.map(|item| item.sim_a.as_str()).unwrap_or(blank);
    let sim_b = scenario.map(|item| item.sim_b.as_str()).unwrap_or(blank);
    obj(vec![
        ("marker_file", s("hidden-truth/README.txt")),
        (
            "alias_cluster",
            obj(vec![
                (
                    "enabled",
                    b(input.smells.enabled("alias_duplicate_customer")),
                ),
                ("keep", s(hx)),
                ("same_tax_id", s(unit)),
                (
                    "do_not_treat_as_two_taxpayers",
                    b(input.smells.enabled("alias_duplicate_customer")),
                ),
            ]),
        ),
        (
            "do_not_merge",
            obj(vec![
                ("enabled", b(input.smells.enabled("similar_not_merge"))),
                ("left", s(sim_a)),
                ("right", s(sim_b)),
                (
                    "reason",
                    s("similar company names with distinct tax identifiers"),
                ),
            ]),
        ),
        (
            "hx_order",
            s(scenario.map(|item| item.hx_ddid.as_str()).unwrap_or(blank)),
        ),
        (
            "hx_waybill",
            s(scenario.map(|item| item.hx_ydh.as_str()).unwrap_or(blank)),
        ),
        (
            "mismatch_order",
            s(scenario
                .map(|item| item.mismatch_ddid.as_str())
                .unwrap_or(blank)),
        ),
        (
            "dangling_waybill",
            s(scenario
                .map(|item| item.dangling_ydh.as_str())
                .unwrap_or(blank)),
        ),
        (
            "west_customer",
            s(scenario.map(|item| item.west.as_str()).unwrap_or(blank)),
        ),
        (
            "prompt_customer",
            s(scenario.map(|item| item.prompt.as_str()).unwrap_or(blank)),
        ),
    ])
}

fn amount_json(cents: u64, integer_fen: bool) -> J {
    if integer_fen {
        u(cents)
    } else {
        s(crate::value::money(i64::try_from(cents).unwrap_or(0), false)
            .expect("scale-2 amount"))
    }
}

pub fn metrics(input: &TruthIn<'_>) -> J {
    let split = input.smells.enabled("metric_invoice_vs_receipt");
    let closing = input.smells.enabled("closing_period");
    let integer_fen = input.smells.enabled("cents");
    let march = if split { 7_550 } else { 17_550 };
    let april = if split {
        u64::try_from(HX_RECEIPT_APRIL_FEN).unwrap_or(0)
    } else {
        0
    };
    let closing_march = if split && !closing { 7_550 } else { 17_550 };
    let rule = if closing {
        "Calendar March is skrq in 2026-03. March closing also takes jzbz=1 receipts dated 2026-04-01 through 2026-04-05."
    } else {
        "Calendar March is skrq in 2026-03. Closing follows the receipt month and does not take April receipts."
    };
    let mut pairs = vec![
        ("order_ref", s(ORDER_REF)),
        ("currency", s("CNY")),
        (
            "invoice_fen",
            amount_json(u64::try_from(HX_INVOICE_FEN).unwrap_or(0), integer_fen),
        ),
        ("invoice_date", s(HX_INVOICE_DATE)),
        (
            "order_fen",
            amount_json(u64::try_from(HX_ORDER_FEN).unwrap_or(0), integer_fen),
        ),
        ("split_enabled", b(split)),
        ("calendar_march_receipt_fen", amount_json(march, integer_fen)),
        (
            "calendar_march_receipt_date",
            s(if split {
                HX_RECEIPT_MARCH_DATE
            } else {
                HX_INVOICE_DATE
            }),
        ),
        ("closing_april_receipt_fen", amount_json(april, integer_fen)),
        ("closing_april_receipt_date", s(HX_RECEIPT_APRIL_DATE)),
        ("closing_april_jzbz", u(if split && closing { 1 } else { 0 })),
        (
            "closing_march_receipt_fen",
            amount_json(closing_march, integer_fen),
        ),
        ("rule", s(rule)),
    ];
    if !integer_fen {
        pairs.push((
            "amount_representation",
            s("fixed_point_yuan_scale_2"),
        ));
    }
    obj(pairs)
}

pub fn calendar(smells: &SmellSet) -> J {
    let fiscal = smells.enabled("fiscal_calendar");
    let closing = smells.enabled("closing_period");
    let example = match (fiscal, closing) {
        (true, true) => "2026-04-02 with jzbz=1 belongs to the March closing and to FY2026-P12.",
        (true, false) => "2026-04-02 is FY2027-P1. It is not pulled into the March closing.",
        (false, true) => {
            "2026-04-02 is FY2026-P04. A jzbz=1 receipt dated 2026-04-01 through 2026-04-05 still belongs to March."
        }
        (false, false) => "2026-04-02 is FY2026-P04. Closing follows the receipt calendar month.",
    };
    obj(vec![
        ("as_of", s(AS_OF)),
        ("business_tz", s(BUSINESS_TZ)),
        (
            "fiscal_start_month",
            u(u64::from(if fiscal { FISCAL_START_MONTH } else { 1 })),
        ),
        (
            "closing_lag_days",
            u(u64::from(if closing { CLOSING_LAG_DAYS } else { 0 })),
        ),
        (
            "fy2026_start",
            s(if fiscal { "2025-04-01" } else { "2026-01-01" }),
        ),
        (
            "fy2026_end",
            s(if fiscal { "2026-03-31" } else { "2026-12-31" }),
        ),
        (
            "march_2026_period",
            s(if fiscal { "FY2026-P12" } else { "FY2026-P03" }),
        ),
        (
            "april_2026_period",
            s(if fiscal { "FY2027-P1" } else { "FY2026-P04" }),
        ),
        ("example", s(example)),
    ])
}

pub fn expected_counts(input: &TruthIn<'_>) -> J {
    let counts = input.counts;
    let shards = input
        .shards
        .iter()
        .map(|(shard, rows)| (format!("{shard}"), u(*rows)))
        .collect::<Vec<_>>();
    obj(vec![
        ("track", u(counts.track)),
        ("customers", u(counts.customers)),
        ("aliases", u(counts.aliases)),
        ("contracts", u(counts.contracts)),
        ("contract_rows", u(counts.contract_rows)),
        ("orders", u(counts.orders)),
        ("receivables", u(counts.receivables)),
        ("receipts", u(counts.receipts)),
        ("refunds", u(counts.refunds)),
        ("invoices", u(counts.invoices)),
        ("waybills", u(counts.waybills)),
        ("lines", u(counts.lines)),
        ("vehicles", u(counts.vehicles)),
        ("drivers", u(counts.drivers)),
        ("routes", u(counts.routes)),
        ("warehouses", u(counts.warehouses)),
        ("skus", u(counts.skus)),
        ("inventory", u(counts.inventory)),
        ("users", u(counts.users)),
        ("dict", u(counts.dict)),
        ("month_end", u(counts.month_end)),
        ("txn_rows", u(counts.txn_rows)),
        ("finance_east_kh", u(input.side.finance_east_kh)),
        ("manager_east_kh", u(input.side.manager_east_kh)),
        ("shards", J::obj(shards)),
    ])
}

pub fn anomalies(input: &TruthIn<'_>) -> J {
    let items = crate::facts::SMELLS
        .iter()
        .map(|name| {
            obj(vec![
                ("smell", s(*name)),
                ("enabled", b(input.smells.enabled(name))),
            ])
        })
        .collect();
    obj(vec![
        ("mismatch_ref", s(MISMATCH_REF)),
        ("order_ref", s(ORDER_REF)),
        ("dangling_token", s("NO-SUCH-ORDER")),
        ("smells", arr(items)),
    ])
}

pub fn invariants(smells: &SmellSet) -> J {
    let cents = smells.enabled("cents");
    let split = smells.enabled("metric_invoice_vs_receipt");
    let closing = smells.enabled("closing_period");
    let waybill = if cents {
        "Waybill header fen equals the order fen when erp_ddh is an order id."
    } else {
        "Waybill header amount equals the order amount when erp_ddh is an order id."
    };
    let lines = if cents {
        "Line fen sums equal the waybill header except the mismatch ref, which is short by 1 fen."
    } else {
        "Line amounts sum to the waybill header except the mismatch ref, which is short by one minor unit."
    };
    let invoice = if split && !closing {
        if cents {
            "The named east order invoice fen is the March calendar receipt plus the April receipt. March closing excludes the April receipt while closing_period is off."
        } else {
            "The named east order invoice amount is the March calendar receipt plus the April receipt. March closing excludes the April receipt while closing_period is off."
        }
    } else if cents {
        "The named east order invoice fen equals the March closing receipt fen."
    } else {
        "The named east order invoice amount equals the March closing receipt amount."
    };
    let diverge = if closing {
        "Calendar March receipts and March closing receipts differ when the receipt split is enabled."
    } else {
        "Calendar March receipts and March closing receipts match while closing_period is off."
    };
    obj(vec![(
        "statements",
        arr(vec![
            s(waybill),
            s(lines),
            s(invoice),
            s(diverge),
            s("A refund skid exists in sk. A delete pk in the workload log does not exist in tk."),
            s(
                "Tracking pos minus seq is constant inside one txn_id and divisible by 100. Epoch is 1.",
            ),
            s("USCC checksum GB 32100-2015 classifies the tax identifiers."),
            s("America/New_York 2026-03-08 02:30 is a gap and 2026-11-01 01:30 is an overlap."),
        ]),
    )])
}

pub fn facts(input: &TruthIn<'_>) -> J {
    obj(vec![
        ("dataset", s(DATASET)),
        ("seed", s(input.seed.to_string())),
        ("partition", s(format!("{:04X}", input.part))),
        ("scale", s(input.scale)),
        ("as_of", s(AS_OF)),
        ("epoch", u(EPOCH)),
        ("prng", s(PRNG_VERSION)),
        ("tzdb", s(input.tzdb)),
        ("business_tz", s(BUSINESS_TZ)),
        ("authz_policy", s(AUTHZ_POLICY)),
        (
            "position_rule",
            s(
                "pos = ordinal * 100 + seq in epoch 1. This is not a binlog position and not a WAL LSN.",
            ),
        ),
    ])
}
