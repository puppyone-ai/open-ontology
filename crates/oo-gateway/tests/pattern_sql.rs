//! Bounded literal LIKE/ILIKE; regular expressions and dynamic patterns stay closed.
use oo_gateway::policy::catalog::PATTERN_OPERATORS;
use oo_gateway::sql_guard::{LIKE_PATTERN_BYTE_LIMIT, LIKE_WILDCARD_LIMIT};
use oo_gateway::{
    GuardError, LiteralTypes, PolicyCatalog, PolicyIdentity, ResolvedOperator, VisibleColumn,
    VisibleRelation, guard_select,
};

fn catalog() -> PolicyCatalog {
    PolicyCatalog {
        identity: PolicyIdentity {
            project: "fixture".into(),
            role: "reader".into(),
            policy_id: "v1".into(),
        },
        relations: vec![VisibleRelation {
            schema: "logistics".into(),
            name: "lookup".into(),
            physical_schema: "_oo".into(),
            physical_name: "pt_lookup".into(),
            columns: [("code", 1043), ("label", 25), ("id", 23), ("fixed", 1042)]
                .into_iter()
                .map(|(name, type_oid)| VisibleColumn {
                    name: name.into(),
                    physical_name: format!("pc_{name}"),
                    type_oid,
                })
                .collect(),
            pending_unclassified: 1,
            estimated_rows: 10,
        }],
        functions: vec![],
        operators: PATTERN_OPERATORS
            .iter()
            .enumerate()
            .map(|(index, name)| ResolvedOperator {
                oid: 300 + index as u32,
                procedure_oid: 400 + index as u32,
                schema: "pg_catalog".into(),
                name: name.to_string(),
                left_type_oid: 25,
                right_type_oid: 25,
                result_type_oid: 16,
            })
            .collect(),
        literal_types: LiteralTypes {
            integer: Some(23),
            text: Some(25),
            boolean: Some(16),
        },
        null_test_allowed: true,
        large_table_row_threshold: 100,
    }
}

#[test]
fn literal_patterns_bind_only_the_requested_catalogued_text_operator() {
    let cat = catalog();
    for (token, name) in [
        ("LIKE", "~~"),
        ("NOT LIKE", "!~~"),
        ("ILIKE", "~~*"),
        ("NOT ILIKE", "!~~*"),
    ] {
        let expected = cat.operators.iter().find(|op| op.name == name).unwrap();
        for column in ["code", "label"] {
            for operator in [
                token.to_string(),
                format!("OPERATOR(pg_catalog.{name})"),
                name.to_string(),
            ] {
                let sql =
                    format!("SELECT id FROM logistics.lookup WHERE {column} {operator} '%a_'");
                let plan = guard_select(&cat.identity, &cat, &sql)
                    .unwrap_or_else(|error| panic!("{sql}: {error:?}"));
                assert_eq!(plan.bindings.operators[0].oid, expected.oid);
                assert_eq!(
                    plan.bindings.operators[0].procedure_oid,
                    expected.procedure_oid
                );
                let mut missing = cat.clone();
                missing.operators.retain(|op| op.name != name);
                assert!(guard_select(&missing.identity, &missing, &sql).is_err());
            }
        }
    }
}

#[test]
fn dynamic_patterns_regex_escape_functions_and_nontext_columns_are_refused() {
    let cat = catalog();
    for sql in [
        "SELECT id FROM logistics.lookup WHERE code LIKE label",
        "SELECT id FROM logistics.lookup WHERE code LIKE lower(label)",
        "SELECT id FROM logistics.lookup WHERE code LIKE '%' ESCAPE ''",
        "SELECT id FROM logistics.lookup WHERE code ILIKE '%' ESCAPE '!'",
        "SELECT id FROM logistics.lookup WHERE code LIKE '%'::text",
        "SELECT id FROM logistics.lookup WHERE code LIKE NULL",
        "SELECT id FROM logistics.lookup WHERE code LIKE 1",
        "SELECT id FROM logistics.lookup WHERE secret LIKE '%'",
        "SELECT id FROM logistics.lookup WHERE lower(code) LIKE '%'",
        "SELECT id FROM logistics.lookup WHERE id LIKE '%'",
        "SELECT id FROM logistics.lookup WHERE fixed LIKE '%'",
        "SELECT id FROM logistics.lookup WHERE code OPERATOR(public.~~) '%'",
        "SELECT id FROM logistics.lookup WHERE code ~ '.*'",
        "SELECT id FROM logistics.lookup WHERE code ~* '.*'",
        "SELECT id FROM logistics.lookup WHERE code SIMILAR TO '%a%'",
        "SELECT id FROM logistics.lookup WHERE code LIKE ANY (ARRAY['a'])",
    ] {
        assert!(guard_select(&cat.identity, &cat, sql).is_err(), "{sql}");
    }
    for mutation in ["schema", "left", "right", "result"] {
        let mut invalid = cat.clone();
        let op = &mut invalid.operators[0];
        match mutation {
            "schema" => op.schema = "public".into(),
            "left" => op.left_type_oid = 1043,
            "right" => op.right_type_oid = 1043,
            "result" => op.result_type_oid = 23,
            _ => unreachable!(),
        }
        assert!(
            guard_select(
                &invalid.identity,
                &invalid,
                "SELECT id FROM logistics.lookup WHERE code LIKE '%'"
            )
            .is_err()
        );
    }
    let mut missing_literal = cat;
    missing_literal.literal_types.text = None;
    assert!(
        guard_select(
            &missing_literal.identity,
            &missing_literal,
            "SELECT id FROM logistics.lookup WHERE code LIKE '%'"
        )
        .is_err()
    );
}

#[test]
fn pattern_budgets_also_cover_operator_syntax_and_escaped_wildcards() {
    let cat = catalog();
    for operator in ["LIKE", "ILIKE", "~~", "OPERATOR(pg_catalog.~~*)"] {
        for pattern in [
            "a".repeat(LIKE_PATTERN_BYTE_LIMIT + 1),
            "_".repeat(LIKE_WILDCARD_LIMIT + 1),
        ] {
            let sql = format!("SELECT id FROM logistics.lookup WHERE code {operator} '{pattern}'");
            assert_eq!(
                guard_select(&cat.identity, &cat, &sql),
                Err(GuardError::Budget)
            );
        }
        for pattern in [
            "a".repeat(LIKE_PATTERN_BYTE_LIMIT),
            "_".repeat(LIKE_WILDCARD_LIMIT),
            "\\%".repeat(LIKE_WILDCARD_LIMIT + 1),
            String::new(),
        ] {
            let sql = format!("SELECT id FROM logistics.lookup WHERE code {operator} '{pattern}'");
            guard_select(&cat.identity, &cat, &sql).unwrap();
        }
    }
    assert!(
        guard_select(
            &cat.identity,
            &cat,
            r"SELECT id FROM logistics.lookup WHERE code LIKE 'a\'"
        )
        .is_err()
    );
}
