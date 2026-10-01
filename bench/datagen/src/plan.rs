use std::collections::BTreeSet;

use crate::error::{Result, err};
use crate::facts::SMELLS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    Xs,
    S,
    M,
    L,
}

impl Scale {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "xs" => Some(Self::Xs),
            "s" => Some(Self::S),
            "m" => Some(Self::M),
            "l" => Some(Self::L),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Xs => "xs",
            Self::S => "s",
            Self::M => "m",
            Self::L => "l",
        }
    }

    pub fn executed(self) -> bool {
        matches!(self, Self::Xs | Self::S)
    }

    /// Largest logical tracking table. `m` and `l` are the SP12 sizes.
    /// This slice does not generate them.
    pub fn track_rows(self) -> u64 {
        match self {
            Self::Xs => 10_000,
            Self::S => 100_000,
            Self::M => 5_000_000,
            Self::L => 50_000_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counts {
    pub track: u64,
    pub customers: u64,
    pub aliases: u64,
    pub contracts: u64,
    pub contract_rows: u64,
    pub orders: u64,
    pub receivables: u64,
    pub receipts: u64,
    pub refunds: u64,
    pub invoices: u64,
    pub waybills: u64,
    pub lines: u64,
    pub vehicles: u64,
    pub drivers: u64,
    pub routes: u64,
    pub warehouses: u64,
    pub skus: u64,
    pub inventory: u64,
    pub users: u64,
    pub dict: u64,
    pub month_end: u64,
    pub txn_rows: u64,
    pub shard_202611: u64,
}

pub fn counts_for(scale: Scale) -> Counts {
    counts_profile(scale, &SmellSet::all_on())
}

pub fn counts_profile(scale: Scale, smells: &SmellSet) -> Counts {
    let alias_extra = smells.enabled("alias_duplicate_customer");
    let metric_split = smells.enabled("metric_invoice_vs_receipt");
    let dst_overlap = smells.enabled("dst_overlap");
    let code_history = smells.enabled("code_meaning_by_time");
    let multi = smells.enabled("multi_table_txn");
    let month_end_on = smells.enabled("month_end_txn");
    let track = scale.track_rows();
    let customers = track / 50;
    let orders = track / 5;
    let contracts = orders / 4;
    let contract_rows = contracts + 1;
    let aliases = customers + u64::from(alias_extra) * 2;
    let vehicles = (track / 200).max(40);
    let drivers = (track / 250).max(32);
    let routes = 24;
    let warehouses = 8;
    let skus = 12;
    let inventory = warehouses * skus * 9;
    let users = 16;
    let mut dict = u64::try_from(crate::catalog::DICT.len()).unwrap_or(0);
    if !code_history {
        dict = dict.saturating_sub(1);
    }
    let month_end = if month_end_on { track / 10 } else { 0 };
    let receipts = receipt_rows(orders, metric_split);
    let invoices = invoice_rows(orders);
    let refunds = refund_rows(orders);
    let ship_rows = if multi {
        orders.saturating_mul(4)
    } else {
        orders
    };
    let txn_rows = ship_rows + month_end + refunds;
    Counts {
        track,
        customers,
        aliases,
        contracts,
        contract_rows,
        orders,
        receivables: orders,
        receipts,
        refunds,
        invoices,
        waybills: orders,
        lines: orders.saturating_mul(3),
        vehicles,
        drivers,
        routes,
        warehouses,
        skus,
        inventory,
        users,
        dict,
        month_end,
        txn_rows,
        shard_202611: u64::from(dst_overlap && track > 1),
    }
}

pub fn receipt_rows(orders: u64, metric_split: bool) -> u64 {
    // Order 0 emits two receipts when the invoice/receipt split is on, else one.
    // Later orders emit one unless index % 10 == 0.
    if orders == 0 {
        return 0;
    }
    let span = orders - 1;
    let first = if metric_split { 2 } else { 1 };
    first + span - span / 10
}

pub fn invoice_rows(orders: u64) -> u64 {
    if orders == 0 {
        return 0;
    }
    let span = orders - 1;
    1 + span - span / 4
}

pub fn emits_receipt(index: u64) -> bool {
    index == 0 || !index.is_multiple_of(10)
}

pub fn receipt_n(index: u64, metric_split: bool) -> u64 {
    if index == 0 {
        u64::from(metric_split) + 1
    } else if !index.is_multiple_of(10) {
        1
    } else {
        0
    }
}

pub fn emits_invoice(index: u64) -> bool {
    index == 0 || !index.is_multiple_of(4)
}

pub fn emits_refund(index: u64) -> bool {
    // Receipts exist when index % 10 != 0. Index % 20 == 11 is inside that set.
    index >= 11 && index % 20 == 11
}

pub fn refund_rows(orders: u64) -> u64 {
    if orders <= 11 {
        return 0;
    }
    (orders - 12) / 20 + 1
}

pub fn active_shards(overlap: bool) -> Vec<u32> {
    let mut shards: Vec<u32> = (202601..=202609).collect();
    if overlap {
        shards.push(202611);
    }
    shards
}

/// Physical shard id `YYYYMM`. Index 1 is the November overlap fixture.
/// Every other row uses month `(index % 9) + 1` in 2026.
pub fn shard_id(index: u64, overlap: bool) -> u32 {
    if overlap && index == 1 {
        202611
    } else {
        202600 + u32::try_from(index % 9).unwrap_or(0) + 1
    }
}

pub fn shard_rows(track: u64, shard: u32, overlap: bool) -> u64 {
    if shard == 202611 {
        return u64::from(overlap && track > 1);
    }
    if !(202601..=202609).contains(&shard) {
        return 0;
    }
    let residue = u64::from(shard - 202601);
    let base = track / 9;
    let rem = track % 9;
    let mut count = base + u64::from(residue < rem);
    if overlap && residue == 1 && track > 1 {
        count -= 1;
    }
    count
}

pub fn shard_list() -> &'static [u32] {
    &[
        202601, 202602, 202603, 202604, 202605, 202606, 202607, 202608, 202609, 202611,
    ]
}

#[derive(Clone, Debug)]
pub struct SmellSet {
    off: BTreeSet<String>,
}

impl SmellSet {
    pub fn all_on() -> Self {
        Self {
            off: BTreeSet::new(),
        }
    }

    pub fn parse_off(names: &[String]) -> Result<Self> {
        let mut off = BTreeSet::new();
        for name in names {
            if !SMELLS.contains(&name.as_str()) {
                return Err(err(format!("unknown bad smell {name}")));
            }
            off.insert(name.clone());
        }
        Ok(Self { off })
    }

    pub fn enabled(&self, name: &str) -> bool {
        !self.off.contains(name)
    }
}

pub fn id(prefix: char, part: u16, index: u64) -> String {
    format!("{prefix}{part:04X}{index:07}")
}

pub fn position(ordinal: u64, seq: u64) -> u64 {
    ordinal.saturating_mul(100).saturating_add(seq)
}

pub fn order_cents(index: u64) -> i64 {
    if index == 0 {
        return crate::facts::HX_ORDER_FEN;
    }
    1_000 + i64::try_from(index % 97).unwrap_or(0) * 13
}

pub fn line_fen(index: u64, mismatch: bool) -> [i64; 3] {
    if index == 0 {
        return crate::facts::HX_LINES_FEN;
    }
    let total = order_cents(index);
    let first = total / 2;
    let second = total / 3;
    let mut third = total - first - second;
    if mismatch && index == 1 {
        third -= 1;
    }
    [first, second, third]
}

#[cfg(test)]
mod tests {
    use super::{Scale, counts_for, invoice_rows, receipt_rows, shard_id, shard_rows};

    #[test]
    fn scale_counts_and_shards_cover_the_track_table() {
        let xs = counts_for(Scale::Xs);
        assert_eq!(xs.track, 10_000);
        assert_eq!(xs.orders, 2_000);
        assert_eq!(xs.lines, 6_000);
        assert_eq!(xs.customers, 200);
        let s = counts_for(Scale::S);
        assert_eq!(s.track, 100_000);
        assert_eq!(s.orders, 20_000);
        assert_eq!(receipt_rows(xs.orders, true), xs.receipts);
        assert_eq!(invoice_rows(xs.orders), xs.invoices);
        let mut sum = 0u64;
        for shard in super::shard_list() {
            sum += shard_rows(xs.track, *shard, true);
        }
        assert_eq!(sum, xs.track);
        let mut plain = 0u64;
        for shard in 202601..=202609 {
            plain += shard_rows(xs.track, shard, false);
        }
        assert_eq!(plain, xs.track);
        assert_eq!(shard_rows(xs.track, 202611, false), 0);
        assert_eq!(shard_id(1, true), 202611);
        assert_eq!(shard_id(1, false), 202602);
        assert_eq!(shard_id(2, true), 202603);
        assert_eq!(shard_id(0, true), 202601);
        assert!(!Scale::M.executed());
        assert_eq!(Scale::L.track_rows(), 50_000_000);
        let txn_off = super::SmellSet::parse_off(&[
            "multi_table_txn".to_string(),
            "month_end_txn".to_string(),
        ])
        .unwrap();
        let quiet = super::counts_profile(Scale::Xs, &txn_off);
        assert_eq!(quiet.month_end, 0);
        assert_eq!(quiet.txn_rows, quiet.orders + quiet.refunds);
        assert_eq!(xs.month_end, 1_000);
        assert_eq!(xs.txn_rows, 9_100);
        assert_eq!(xs.refunds, 100);
        assert_eq!(s.refunds, 1_000);
        assert!(super::emits_receipt(11));
        assert!(super::emits_refund(11));
        assert!(!super::emits_receipt(10));
        assert!(!super::emits_refund(10));
        assert_eq!(super::active_shards(false).len(), 9);
        assert_eq!(super::active_shards(true).len(), 10);
    }
}
