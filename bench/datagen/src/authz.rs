//! Synthetic role claims. Allow and deny are policy expectations, not a
//! security runtime pass.

use crate::facts::{
    AUTHZ_POLICY, NAME_HX, NAME_SOFT, NAME_SOFT_OFF, NAME_TEST, NAME_TEST_OFF, NAME_WEST,
    PII_PHONE, PII_SFZH,
};
use crate::jsonutil::{J, arr, b, obj, s};
use crate::plan::SmellSet;

struct Role {
    name: &'static str,
    allow: &'static [&'static str],
    deny: &'static [&'static str],
    columns_deny: &'static [&'static str],
    szdq: &'static str,
    csbz: &'static str,
    scbz: &'static str,
    ckid: &'static str,
    raw_pii: &'static str,
}

const ROLES: &[Role] = &[
    Role {
        name: "dispatcher",
        allow: &[
            "yd", "yd_mx", "gj", "cl", "sj", "xl", "ck", "kh", "kh_bm", "zd",
        ],
        deny: &["ht", "dd", "ys", "sk", "tk", "fp", "yh", "kc"],
        columns_deny: &[
            "sfzh", "sjhm", "lxdh", "nsrsbh", "je_fen", "skje_fen", "fpje_fen", "tkje_fen", "hlje",
            "ysje_fen",
        ],
        szdq: "*",
        csbz: "*",
        scbz: "*",
        ckid: "*",
        raw_pii: "deny",
    },
    Role {
        name: "finance_east",
        allow: &["kh", "kh_bm", "ht", "dd", "ys", "sk", "tk", "fp", "zd"],
        deny: &["yd", "yd_mx", "gj", "cl", "sj", "xl", "ck", "kc", "yh"],
        columns_deny: &["sfzh", "sjhm", "lxdh"],
        szdq: "east",
        csbz: "0",
        scbz: "*",
        ckid: "*",
        raw_pii: "deny",
    },
    Role {
        name: "finance_west",
        allow: &["kh", "kh_bm", "ht", "dd", "ys", "sk", "tk", "fp", "zd"],
        deny: &["yd", "yd_mx", "gj", "cl", "sj", "xl", "ck", "kc", "yh"],
        columns_deny: &["sfzh", "sjhm", "lxdh"],
        szdq: "west",
        csbz: "0",
        scbz: "*",
        ckid: "*",
        raw_pii: "deny",
    },
    Role {
        name: "manager_east",
        allow: &["kh", "kh_bm", "ht", "dd", "ys", "sk", "tk", "fp", "zd"],
        deny: &["yd", "yd_mx", "gj", "cl", "sj", "xl", "ck", "kc", "yh"],
        columns_deny: &["sfzh", "sjhm", "lxdh"],
        szdq: "east",
        csbz: "0",
        scbz: "0",
        ckid: "*",
        raw_pii: "deny",
    },
    Role {
        name: "warehouse",
        allow: &["ck", "kc", "yd", "yd_mx", "zd"],
        deny: &[
            "kh", "ht", "dd", "ys", "sk", "tk", "fp", "sj", "yh", "kh_bm", "gj", "cl", "xl",
        ],
        columns_deny: &["sfzh", "sjhm", "lxdh", "nsrsbh"],
        szdq: "*",
        csbz: "*",
        scbz: "*",
        ckid: "CK01",
        raw_pii: "deny",
    },
    Role {
        name: "auditor",
        allow: &[
            "kh", "kh_bm", "ht", "dd", "ys", "sk", "tk", "fp", "zd", "yh", "yd", "yd_mx", "gj",
            "cl", "sj", "xl", "ck", "kc",
        ],
        deny: &[],
        columns_deny: &["sfzh", "sjhm", "lxdh"],
        szdq: "*",
        csbz: "*",
        scbz: "*",
        ckid: "*",
        raw_pii: "deny",
    },
];

pub fn role_names() -> Vec<&'static str> {
    ROLES.iter().map(|role| role.name).collect()
}

pub fn roles_json() -> J {
    let roles = ROLES
        .iter()
        .map(|role| {
            obj(vec![
                ("role", s(role.name)),
                (
                    "tables_allow",
                    arr(role.allow.iter().map(|name| s(*name)).collect()),
                ),
                (
                    "tables_deny",
                    arr(role.deny.iter().map(|name| s(*name)).collect()),
                ),
                (
                    "columns_deny",
                    arr(role.columns_deny.iter().map(|name| s(*name)).collect()),
                ),
                (
                    "kh_row",
                    obj(vec![
                        ("szdq", s(role.szdq)),
                        ("csbz", s(role.csbz)),
                        ("scbz", s(role.scbz)),
                    ]),
                ),
                ("ckid", s(role.ckid)),
                ("raw_pii", s(role.raw_pii)),
                ("canaries", arr(vec![s(PII_PHONE), s(PII_SFZH)])),
            ])
        })
        .collect();
    obj(vec![
        ("policy", s(AUTHZ_POLICY)),
        ("runtime_executed", b(false)),
        ("roles", arr(roles)),
    ])
}

pub fn matrix_json() -> J {
    let mut cells = Vec::new();
    for role in ROLES {
        for table in role.allow {
            cells.push(cell(role.name, table, "allow"));
        }
        for table in role.deny {
            cells.push(cell(role.name, table, "deny"));
        }
    }
    obj(vec![("policy", s(AUTHZ_POLICY)), ("cells", arr(cells))])
}

fn cell(role: &str, table: &str, decision: &str) -> J {
    obj(vec![
        ("role", s(role)),
        ("table", s(table)),
        ("decision", s(decision)),
    ])
}

pub fn decisions_json(smells: &SmellSet) -> J {
    let soft = if smells.enabled("soft_delete") {
        NAME_SOFT
    } else {
        NAME_SOFT_OFF
    };
    let soft_scbz = if smells.enabled("soft_delete") {
        "1"
    } else {
        "0"
    };
    let test = if smells.enabled("test_customer") {
        NAME_TEST
    } else {
        NAME_TEST_OFF
    };
    let test_csbz = if smells.enabled("test_customer") {
        "1"
    } else {
        "0"
    };
    let cases = vec![
        kh("AUTH-EAST-HX", "finance_east", NAME_HX, "east", "0", "0"),
        kh(
            "AUTH-WEST-ON-EAST",
            "finance_east",
            NAME_WEST,
            "west",
            "0",
            "0",
        ),
        kh(
            "AUTH-WEST-ON-WEST",
            "finance_west",
            NAME_WEST,
            "west",
            "0",
            "0",
        ),
        kh("AUTH-MGR-HX", "manager_east", NAME_HX, "east", "0", "0"),
        kh("AUTH-MGR-WEST", "manager_east", NAME_WEST, "west", "0", "0"),
        kh(
            "AUTH-EAST-SOFT",
            "finance_east",
            soft,
            "east",
            "0",
            soft_scbz,
        ),
        kh(
            "AUTH-MGR-SOFT",
            "manager_east",
            soft,
            "east",
            "0",
            soft_scbz,
        ),
        kh(
            "AUTH-EAST-TEST",
            "finance_east",
            test,
            "east",
            test_csbz,
            "0",
        ),
        fixed(
            "AUTH-WH-CK01",
            "warehouse",
            "CK01",
            "allow",
            "home warehouse",
        ),
        fixed(
            "AUTH-WH-CK02",
            "warehouse",
            "CK02",
            "deny",
            "other warehouse",
        ),
        fixed("AUTH-AUD-RAW", "auditor", "sfzh", "deny", "raw pii"),
        fixed("AUTH-AUD-NAME", "auditor", "xm", "allow", "non-pii column"),
        fixed(
            "AUTH-DISP-FIN",
            "dispatcher",
            "skje_fen",
            "deny",
            "finance column",
        ),
        fixed(
            "AUTH-DISP-YD",
            "dispatcher",
            "yd",
            "allow",
            "dispatch table",
        ),
    ];
    obj(vec![
        ("policy", s(AUTHZ_POLICY)),
        ("runtime_executed", b(false)),
        ("cases", arr(cases)),
    ])
}

fn kh(id: &str, role: &str, subject: &str, szdq: &str, csbz: &str, scbz: &str) -> J {
    let decision = kh_decision(role, szdq, csbz, scbz);
    obj(vec![
        ("id", s(id)),
        ("role", s(role)),
        ("subject", s(subject)),
        ("kind", s("customer")),
        ("decision", s(decision)),
        ("reason", s(format!("szdq={szdq} csbz={csbz} scbz={scbz}"))),
    ])
}

fn kh_decision(role: &str, szdq: &str, csbz: &str, scbz: &str) -> &'static str {
    let allow = match role {
        "finance_east" => szdq == "east" && csbz == "0",
        "finance_west" => szdq == "west" && csbz == "0",
        "manager_east" => szdq == "east" && csbz == "0" && scbz == "0",
        _ => false,
    };
    if allow { "allow" } else { "deny" }
}

fn fixed(id: &str, role: &str, subject: &str, decision: &str, reason: &str) -> J {
    obj(vec![
        ("id", s(id)),
        ("role", s(role)),
        ("subject", s(subject)),
        ("kind", s("fixed")),
        ("decision", s(decision)),
        ("reason", s(reason)),
    ])
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{matrix_json, roles_json};
    use crate::catalog::BUSINESS_TABLES;
    use crate::jsonutil::J;

    fn str_field(obj: &J, key: &str) -> String {
        let J::Obj(pairs) = obj else {
            panic!("object");
        };
        let J::Str(value) = &pairs.iter().find(|(name, _)| name == key).unwrap().1 else {
            panic!("{key}");
        };
        value.clone()
    }

    #[test]
    fn every_role_allow_and_deny_cover_the_18_tables() {
        let rendered = roles_json().render().unwrap();
        assert!(rendered.contains("\"runtime_executed\":false"));
        assert!(!rendered.contains("model_verdict"));
        let J::Obj(root) = matrix_json() else {
            panic!("matrix");
        };
        let J::Arr(cells) = &root.iter().find(|(key, _)| key == "cells").unwrap().1 else {
            panic!("cells");
        };
        assert_eq!(cells.len(), BUSINESS_TABLES.len() * 6);
        let mut by_role: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
        for cell in cells {
            let role = str_field(cell, "role");
            let table = str_field(cell, "table");
            let decision = str_field(cell, "decision");
            let entry = by_role.entry(role).or_default();
            let inserted = if decision == "allow" {
                entry.0.insert(table)
            } else {
                assert_eq!(decision, "deny");
                entry.1.insert(table)
            };
            assert!(inserted, "duplicate cell");
        }
        assert_eq!(by_role.len(), 6);
        let business: BTreeSet<String> = BUSINESS_TABLES
            .iter()
            .map(|name| (*name).to_string())
            .collect();
        for (role, (allow, deny)) in &by_role {
            assert!(allow.is_disjoint(deny), "{role}");
            let union: BTreeSet<String> = allow.union(deny).cloned().collect();
            assert_eq!(&union, &business, "{role}");
        }
        let dispatcher = by_role.get("dispatcher").unwrap();
        assert!(dispatcher.0.contains("zd"));
        assert!(dispatcher.1.contains("kc"));
        assert!(dispatcher.1.contains("ys"));
        let warehouse = by_role.get("warehouse").unwrap();
        assert!(warehouse.0.contains("zd"));
        assert!(warehouse.0.contains("kc"));
        for denied in ["kh_bm", "gj", "cl", "xl"] {
            assert!(warehouse.1.contains(denied), "{denied}");
        }
        assert_eq!(warehouse.0.len() + warehouse.1.len(), 18);
    }
}
