//! CC-01 through CC-13. A runtime claim needs every case that applies to it.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applies {
    Always,
    Caps(&'static [&'static str]),
    DocumentFamily,
}

pub struct CaseSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub applies: Applies,
}

pub static CASES: &[CaseSpec] = &[
    CaseSpec { id: "CC-01", name: "claim honesty", applies: Applies::Always },
    CaseSpec { id: "CC-02", name: "source zero-write audit", applies: Applies::Always },
    CaseSpec {
        id: "CC-03",
        name: "type fidelity",
        applies: Applies::Caps(&["catalog", "batch_full", "batch_incremental", "cdc"]),
    },
    CaseSpec { id: "CC-04", name: "chunk coverage", applies: Applies::Caps(&["batch_full"]) },
    CaseSpec {
        id: "CC-05",
        name: "cursor and overlap",
        applies: Applies::Caps(&["batch_incremental"]),
    },
    CaseSpec {
        id: "CC-06",
        name: "change read and expiry",
        applies: Applies::Caps(&["change_feed", "cdc"]),
    },
    CaseSpec {
        id: "CC-07",
        name: "delete convergence",
        applies: Applies::Caps(&["delete", "delete_native", "delete_reconcile"]),
    },
    CaseSpec { id: "CC-08", name: "cancel and budget", applies: Applies::Caps(&["budget"]) },
    CaseSpec {
        id: "CC-09",
        name: "fault and recovery",
        applies: Applies::Caps(&["cdc", "change_feed"]),
    },
    CaseSpec { id: "CC-10", name: "least privilege", applies: Applies::Caps(&["catalog"]) },
    CaseSpec { id: "CC-11", name: "driver and supply chain", applies: Applies::Always },
    CaseSpec { id: "CC-12", name: "semantic propagation", applies: Applies::Caps(&["catalog"]) },
    CaseSpec { id: "CC-13", name: "document mapping", applies: Applies::DocumentFamily },
];

pub const CERTIFIED_CASES: &[&str] = &[
    "FAULT-POSITION-EXPIRED",
    "FAULT-FAILOVER",
    "FAULT-DDL",
    "SCALE-S100K",
    "SCALE-5M",
];

pub fn required_cases(family: &str, capability: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    for case in CASES {
        let hit = match case.applies {
            Applies::Always => true,
            Applies::Caps(names) => names.contains(&capability),
            Applies::DocumentFamily => family == "document" || capability == "document",
        };
        if hit {
            out.push(case.id);
        }
    }
    out
}

pub fn canonical_capability(raw: &str) -> Option<&'static str> {
    Some(match raw {
        "cat" | "catalog" => "catalog",
        "logic" => "logic",
        "qlog" | "query_log" => "query_log",
        "full" | "batch_full" => "batch_full",
        "inc" | "batch_incremental" => "batch_incremental",
        "feed" | "change_feed" => "change_feed",
        "cdc" => "cdc",
        "del" | "delete" => "delete",
        "delete_native" => "delete_native",
        "delete_reconcile" => "delete_reconcile",
        "snap" | "snapshot_token" => "snapshot_token",
        "rep" | "replica_read" => "replica_read",
        "bud" | "budget" => "budget",
        "document" => "document",
        _ => return None,
    })
}
