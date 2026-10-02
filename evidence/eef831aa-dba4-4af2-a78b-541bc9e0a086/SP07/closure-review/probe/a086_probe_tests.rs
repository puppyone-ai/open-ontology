use std::collections::BTreeSet;

const CANARY_NAME: &str = "a086_name_7c2e";
const CANARY_COMMENT: &str = "a086_comment_7c2e";

fn evidence_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("OO_A086_EVIDENCE").unwrap_or_else(|_| "/tmp/a086-probe".to_string()),
    )
}

fn note(name: &str, ok: bool, detail: &str) {
    let dir = evidence_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("cases.jsonl"))
        .expect("probe log");
    use std::io::Write;
    let _ = writeln!(
        file,
        "{}",
        json!({"name": name, "ok": ok, "detail": detail})
    );
}

fn record(failures: &mut Vec<String>, name: &str, ok: bool, detail: impl AsRef<str>) {
    let detail = detail.as_ref();
    note(name, ok, detail);
    if !ok {
        failures.push(format!("{name}: {detail}"));
    }
}

fn dataset_fingerprint(schema: &str, name: &str) -> String {
    crate::canonical::canonical_sha256(&json!({"kind": "dataset", "name": name, "schema": schema}))
}

fn column_fingerprint_of(draft: &Value, item_id: &str) -> Value {
    queue_item(draft, item_id, "structural_column")["fact_fingerprint"].clone()
}

fn relationship_fingerprint_of(draft: &Value, item_id: &str) -> Value {
    queue_item(draft, item_id, "candidate_relationship")["fact_fingerprint"].clone()
}

fn reject_column(root: &mut Value, draft: &Value, item_id: &str, dataset: &str, column: &str) {
    let schema_hash = draft["provenance"]["schema_hash"].clone();
    let review = json!({
        "item_id": item_id,
        "action": "reject",
        "schema_hash": schema_hash,
        "fact_fingerprint": column_fingerprint_of(draft, item_id),
    });
    push_review(root, review);
    push_curated_column(root, dataset, column);
}

fn push_review(root: &mut Value, review: Value) {
    if !root["reviews"].is_array() {
        root["reviews"] = json!([]);
    }
    root["reviews"].as_array_mut().unwrap().push(review);
}

fn push_curated_column(root: &mut Value, dataset: &str, column: &str) {
    if root["curated"].is_null() {
        root["curated"] = json!({"datasets": [], "relationships": []});
    }
    let datasets = root["curated"]["datasets"].as_array_mut().unwrap();
    if let Some(slot) = datasets.iter_mut().find(|item| item["dataset"] == dataset) {
        slot["columns"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": column, "status": "rejected", "label": "kept-label"}));
    } else {
        datasets.push(json!({
            "dataset": dataset,
            "columns": [{"name": column, "status": "rejected", "label": "kept-label"}]
        }));
    }
}

fn refresh_selection(root: &mut Value) {
    let tables = root["catalog"]["tables"].as_array().cloned().unwrap_or_default();
    root["selection"] = selection_for(&tables);
}

fn add_column(root: &mut Value, table_name: &str, column: Value) {
    for table in root["catalog"]["tables"].as_array_mut().unwrap() {
        if table["name"] == table_name {
            table["columns"].as_array_mut().unwrap().push(column.clone());
        }
    }
    refresh_selection(root);
}

fn names_of(draft: &Value, layer: &str) -> BTreeSet<String> {
    draft[layer]["relationships"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap().to_string())
        .collect()
}

fn effective_holes(draft: &Value) -> Vec<String> {
    let mut index = BTreeMap::new();
    for dataset in draft["effective"]["datasets"].as_array().unwrap() {
        let name = dataset["dataset"].as_str().unwrap().to_string();
        let columns = dataset["columns"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|column| column["name"].as_str().map(str::to_string))
            .collect::<BTreeSet<_>>();
        index.insert(name, columns);
    }
    let mut holes = Vec::new();
    for relationship in draft["effective"]["relationships"].as_array().unwrap() {
        let name = relationship["name"].as_str().unwrap();
        for side in ["from", "to"] {
            let dataset = relationship[side]["dataset"].as_str().unwrap();
            match index.get(dataset) {
                None => holes.push(format!("{name} {side} dataset {dataset}")),
                Some(columns) => {
                    for column in relationship[side]["columns"].as_array().unwrap() {
                        let column = column.as_str().unwrap();
                        if !columns.contains(column) {
                            holes.push(format!("{name} {side} column {dataset}.{column}"));
                        }
                    }
                }
            }
        }
    }
    holes
}

fn unpublished(draft: &Value) -> bool {
    draft["provenance"]["publication"] == "unavailable"
        && draft["provenance"]["llm"] == "not_used"
        && draft["provenance"]["certification"] == "none"
        && draft["provenance"]["pg_catalog_integration"] == "not_accepted"
        && draft["review_queue"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["publishable"] == false)
        && draft["physical"]["tables"].as_array().unwrap().iter().all(|table| {
            table["columns"].as_array().unwrap().iter().all(|column| {
                column["type_support"]["converted"] == false
            })
        })
}

fn guard(failures: &mut Vec<String>, name: &str, draft: &Value) {
    let holes = effective_holes(draft);
    record(
        failures,
        &format!("{name}/effective-endpoints"),
        holes.is_empty(),
        holes.join("; "),
    );
    record(
        failures,
        &format!("{name}/unpublished"),
        unpublished(draft),
        "publication, llm, certification, integration, publishable, or type conversion changed",
    );
}

fn closure_detail(draft: &Value, item_id: &str) -> String {
    queue_item_opt(draft, item_id, "relationship_endpoint_not_effective")
        .and_then(|item| item["detail"].as_str().map(str::to_string))
        .unwrap_or_default()
}

fn probe_orders() -> Vec<Value> {
    let customer = table(
        "customer",
        vec![int4(1, "id", true, None), int4(2, "name", false, None)],
        vec![key_col(1, "id")],
        Some(false),
        Vec::new(),
        Vec::new(),
    );
    let warehouse = table(
        "warehouse",
        vec![int4(1, "id", true, None)],
        vec![key_col(1, "id")],
        Some(false),
        Vec::new(),
        Vec::new(),
    );
    let orders = table(
        "orders",
        vec![
            int4(1, "id", true, None),
            int4(2, "customer_id", true, None),
            int4(3, "ship_to", true, None),
        ],
        vec![key_col(1, "id")],
        Some(false),
        Vec::new(),
        vec![
            fk_one("fk_bill", 2, "customer_id", "customer", 1, "id"),
            fk_one("fk_alt", 2, "customer_id", "customer", 1, "id"),
            fk_one("fk_ship", 3, "ship_to", "warehouse", 1, "id"),
        ],
    );
    vec![customer, warehouse, orders]
}

fn write_envelope(name: &str, bytes: &str) {
    let dir = evidence_dir().join("cli-input");
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(dir.join(name), bytes).expect("write envelope");
}

fn static_error(bytes: &str, suffix: &str, absent: &[&str]) -> Result<(), String> {
    let err = compile_structure_draft(bytes).map(|_| "compiled".to_string()).unwrap_err();
    if !err.code.ends_with(suffix) {
        return Err(format!("code {} path {} message {}", err.code, err.path, err.message));
    }
    let text = err.to_string();
    for token in absent {
        if text.contains(token) {
            return Err(format!("leaked {token} in {text}"));
        }
    }
    if err.message.chars().any(|ch| ch.is_control()) {
        return Err("message contains a control".to_string());
    }
    Ok(())
}

fn check_fixtures(failures: &mut Vec<String>) {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/builder/structure-draft/fixtures");
    let mut names: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|item| item.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    names.sort();
    record(
        failures,
        "fixture-count",
        names.len() == 8,
        format!("found {}", names.len()),
    );
    for path in names {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(&path).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        let parsed = parse_catalog(&value["catalog"]);
        let Ok(parsed) = parsed else {
            record(failures, &format!("fixture/{name}/parse"), false, "parse_catalog rejected a checked-in fixture");
            continue;
        };
        let recomputed = crate::pgsql::schema_hash(&parsed.tables);
        let selection = super::SelectionIn::deserialize_selection(&value["selection"]);
        let bindings = json!({
            "schema_hash": parsed.schema_hash,
            "config_hash": config_hash(&parsed),
            "type_hash": type_hash(&parsed),
            "evidence_hash": evidence_hash(&parsed, &selection),
        });
        let hash_ok = value["catalog"]["schema_hash"] == parsed.schema_hash
            && parsed.schema_hash == recomputed
            && value["bindings"] == bindings;
        record(
            failures,
            &format!("fixture/{name}/recomputed-hash"),
            hash_ok,
            "catalog or binding hash does not match an independent recompute",
        );
        match compile_structure_draft(&text) {
            Ok(output) => {
                let draft: Value = serde_json::from_str(&output).unwrap();
                guard(failures, &format!("fixture/{name}"), &draft);
                record(
                    failures,
                    &format!("fixture/{name}/compile"),
                    true,
                    "compiled",
                );
            }
            Err(err) => {
                let duplicate = name.starts_with("duplicate-");
                record(
                    failures,
                    &format!("fixture/{name}/compile"),
                    duplicate && err.code.ends_with("/duplicate.key_member"),
                    err.to_string(),
                );
                record(
                    failures,
                    &format!("fixture/{name}/no-leak"),
                    !err.to_string().contains("leak_pk_9f3a")
                        && !err.to_string().contains("DO_NOT_SURFACE"),
                    err.to_string(),
                );
            }
        }
    }
}

fn closure_cases(failures: &mut Vec<String>) {
    let base = compile(envelope(probe_orders()));
    guard(failures, "baseline", &base);
    let open = names_of(&base, "effective");
    record(
        failures,
        "baseline/three-fks",
        open == BTreeSet::from(["fk_alt".to_string(), "fk_bill".to_string(), "fk_ship".to_string()]),
        format!("{open:?}"),
    );

    let mut shared = envelope(probe_orders());
    let first = compile(shared.clone());
    reject_column(
        &mut shared,
        &first,
        "column/public.orders/customer_id",
        "public.orders",
        "customer_id",
    );
    let shared_draft = compile(shared);
    guard(failures, "shared-source", &shared_draft);
    let left = names_of(&shared_draft, "effective");
    record(
        failures,
        "shared-source/only-ship-remains",
        left == BTreeSet::from(["fk_ship".to_string()]),
        format!("{left:?}"),
    );
    record(
        failures,
        "shared-source/generated-kept",
        names_of(&shared_draft, "generated").len() == 3,
        "generated relationships changed",
    );
    for name in ["fk_bill", "fk_alt"] {
        let item = format!("relationship/public.orders/{name}");
        record(
            failures,
            &format!("shared-source/{name}"),
            closure_detail(&shared_draft, &item) == "from_column_not_effective"
                && queue_item(&shared_draft, &item, "relationship_endpoint_not_effective")["state"] == "pending"
                && queue_item(&shared_draft, &item, "relationship_endpoint_not_effective")["publishable"] == false,
            closure_detail(&shared_draft, &item),
        );
    }
    record(
        failures,
        "shared-source/history",
        shared_draft["reviews"][0]["action"] == "reject"
            && curated_column(&shared_draft, "public.orders", "customer_id")["label"] == "kept-label"
            && curated_column(&shared_draft, "public.orders", "customer_id")["status"] == "rejected",
        "review or curated echo changed",
    );

    let mut both = envelope(probe_orders());
    let first = compile(both.clone());
    reject_column(&mut both, &first, "column/public.orders/customer_id", "public.orders", "customer_id");
    reject_column(&mut both, &first, "column/public.customer/id", "public.customer", "id");
    let both_draft = compile(both);
    guard(failures, "both-ends", &both_draft);
    record(
        failures,
        "both-ends/detail",
        closure_detail(&both_draft, "relationship/public.orders/fk_bill") == "from_and_to_column_not_effective"
            && closure_detail(&both_draft, "relationship/public.orders/fk_alt") == "from_and_to_column_not_effective"
            && names_of(&both_draft, "effective") == BTreeSet::from(["fk_ship".to_string()]),
        format!(
            "bill={} alt={} left={:?}",
            closure_detail(&both_draft, "relationship/public.orders/fk_bill"),
            closure_detail(&both_draft, "relationship/public.orders/fk_alt"),
            names_of(&both_draft, "effective")
        ),
    );
    record(
        failures,
        "both-ends/customer-key-stripped",
        dataset(&both_draft, "effective", "public.customer")["primary_key"]
            .as_array()
            .unwrap()
            .is_empty()
            && column_of(dataset(&both_draft, "effective", "public.customer"), "name").is_some(),
        "customer primary key or unrelated column changed",
    );

    let mut all_customer = envelope(probe_orders());
    let first = compile(all_customer.clone());
    reject_column(&mut all_customer, &first, "column/public.customer/id", "public.customer", "id");
    reject_column(&mut all_customer, &first, "column/public.customer/name", "public.customer", "name");
    let all_draft = compile(all_customer);
    guard(failures, "all-customer-columns", &all_draft);
    record(
        failures,
        "all-customer-columns/fks",
        closure_detail(&all_draft, "relationship/public.orders/fk_bill") == "to_column_not_effective"
            && closure_detail(&all_draft, "relationship/public.orders/fk_alt") == "to_column_not_effective"
            && names_of(&all_draft, "effective") == BTreeSet::from(["fk_ship".to_string()])
            && dataset(&all_draft, "effective", "public.customer")["columns"]
                .as_array()
                .unwrap()
                .is_empty(),
        format!("{:?}", names_of(&all_draft, "effective")),
    );

    let mut accept_dataset = envelope(probe_orders());
    let first = compile(accept_dataset.clone());
    let schema_hash = first["provenance"]["schema_hash"].clone();
    push_review(
        &mut accept_dataset,
        json!({
            "item_id": "dataset/public.orders",
            "action": "accept",
            "schema_hash": schema_hash,
            "fact_fingerprint": dataset_fingerprint("public", "orders"),
        }),
    );
    reject_column(
        &mut accept_dataset,
        &first,
        "column/public.orders/customer_id",
        "public.orders",
        "customer_id",
    );
    let accepted = compile(accept_dataset);
    guard(failures, "accept-dataset", &accepted);
    record(
        failures,
        "accept-dataset/does-not-restore",
        column_of(dataset(&accepted, "effective", "public.orders"), "customer_id").is_none()
            && names_of(&accepted, "effective") == BTreeSet::from(["fk_ship".to_string()])
            && queue_item_opt(&accepted, "dataset/public.orders", "stale_decision").is_none()
            && accepted["reviews"].as_array().unwrap().iter().any(|review| {
                review["item_id"] == "dataset/public.orders" && review["action"] == "accept"
            }),
        "dataset accept restored the column, staled, or dropped the decision",
    );

    let mut accept_sibling = envelope(probe_orders());
    let first = compile(accept_sibling.clone());
    let schema_hash = first["provenance"]["schema_hash"].clone();
    push_review(
        &mut accept_sibling,
        json!({
            "item_id": "column/public.orders/ship_to",
            "action": "accept",
            "schema_hash": schema_hash,
            "fact_fingerprint": column_fingerprint_of(&first, "column/public.orders/ship_to"),
        }),
    );
    reject_column(
        &mut accept_sibling,
        &first,
        "column/public.orders/customer_id",
        "public.orders",
        "customer_id",
    );
    let sibling = compile(accept_sibling);
    guard(failures, "accept-sibling", &sibling);
    record(
        failures,
        "accept-sibling/does-not-restore",
        column_of(dataset(&sibling, "effective", "public.orders"), "ship_to").is_some()
            && column_of(dataset(&sibling, "effective", "public.orders"), "customer_id").is_none()
            && queue_item(&sibling, "column/public.orders/ship_to", "structural_column")["state"] == "accepted"
            && names_of(&sibling, "effective") == BTreeSet::from(["fk_ship".to_string()]),
        format!("{:?}", names_of(&sibling, "effective")),
    );

    let mut labeled = envelope(probe_orders());
    let first = compile(labeled.clone());
    let schema_hash = first["provenance"]["schema_hash"].clone();
    push_review(
        &mut labeled,
        json!({
            "item_id": "dataset/public.orders",
            "action": "override",
            "schema_hash": schema_hash,
            "fact_fingerprint": dataset_fingerprint("public", "orders"),
        }),
    );
    reject_column(&mut labeled, &first, "column/public.orders/customer_id", "public.orders", "customer_id");
    labeled["curated"]["datasets"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["dataset"] == "public.orders")
        .unwrap()["label"] = json!("Orders label");
    let labeled_draft = compile(labeled);
    guard(failures, "override-label", &labeled_draft);
    record(
        failures,
        "override-label/does-not-restore",
        dataset(&labeled_draft, "effective", "public.orders")["label"] == "Orders label"
            && column_of(dataset(&labeled_draft, "effective", "public.orders"), "customer_id").is_none()
            && names_of(&labeled_draft, "effective") == BTreeSet::from(["fk_ship".to_string()]),
        "label override restored the rejected endpoint",
    );

    let mut restored = envelope(probe_orders());
    let first = compile(restored.clone());
    reject_column(
        &mut restored,
        &first,
        "column/public.orders/customer_id",
        "public.orders",
        "customer_id",
    );
    let closed = compile(restored.clone());
    add_column(&mut restored, "orders", int4(4, "note", false, None));
    let restored_draft = compile(restored);
    guard(failures, "restored-version", &restored_draft);
    record(
        failures,
        "restored-version/endpoint-returns",
        names_of(&closed, "effective") == BTreeSet::from(["fk_ship".to_string()])
            && names_of(&restored_draft, "effective").len() == 3
            && column_of(dataset(&restored_draft, "effective", "public.orders"), "customer_id").is_some()
            && column_of(dataset(&restored_draft, "effective", "public.orders"), "note").is_some()
            && queue_item_opt(
                &restored_draft,
                "relationship/public.orders/fk_bill",
                "relationship_endpoint_not_effective",
            )
            .is_none()
            && queue_item(&restored_draft, "column/public.orders/customer_id", "stale_decision")["detail"]
                == "stale_schema_hash"
            && restored_draft["reviews"][0]["action"] == "reject"
            && curated_column(&restored_draft, "public.orders", "customer_id")["status"] == "rejected",
        format!("closed={:?} restored={:?}", names_of(&closed, "effective"), names_of(&restored_draft, "effective")),
    );

    let mut stale_rel = envelope(probe_orders());
    let first = compile(stale_rel.clone());
    let item = "relationship/public.orders/fk_ship";
    push_review(
        &mut stale_rel,
        json!({
            "item_id": item,
            "action": "reject",
            "schema_hash": first["provenance"]["schema_hash"],
            "fact_fingerprint": relationship_fingerprint_of(&first, item),
        }),
    );
    stale_rel["curated"] = json!({
        "datasets": [],
        "relationships": [{"name": "fk_ship", "status": "rejected"}]
    });
    add_column(&mut stale_rel, "orders", int4(4, "note", false, None));
    let stale_draft = compile(stale_rel);
    guard(failures, "stale-relationship", &stale_draft);
    record(
        failures,
        "stale-relationship/removes-nothing",
        names_of(&stale_draft, "effective").len() == 3
            && queue_item_opt(&stale_draft, item, "relationship_endpoint_not_effective").is_none()
            && queue_item(&stale_draft, item, "stale_decision")["detail"] == "stale_schema_hash"
            && queue_item(&stale_draft, item, "candidate_relationship")["state"] == "pending"
            && stale_draft["reviews"][0]["action"] == "reject",
        format!("{:?}", names_of(&stale_draft, "effective")),
    );

    let mut human = envelope(probe_orders());
    let first = compile(human.clone());
    let item = "relationship/public.orders/fk_ship";
    push_review(
        &mut human,
        json!({
            "item_id": item,
            "action": "reject",
            "schema_hash": first["provenance"]["schema_hash"],
            "fact_fingerprint": relationship_fingerprint_of(&first, item),
        }),
    );
    human["curated"] = json!({
        "datasets": [],
        "relationships": [{"name": "fk_ship", "status": "rejected"}]
    });
    let human_draft = compile(human.clone());
    guard(failures, "human-reject", &human_draft);
    record(
        failures,
        "human-reject/no-spurious-endpoint",
        names_of(&human_draft, "effective") == BTreeSet::from(["fk_alt".to_string(), "fk_bill".to_string()])
            && queue_item_opt(&human_draft, item, "relationship_endpoint_not_effective").is_none()
            && queue_item(&human_draft, item, "candidate_relationship")["state"] == "rejected"
            && human_draft["reviews"][0]["fact_fingerprint"] == relationship_fingerprint_of(&first, item)
            && human_draft["curated"]["relationships"][0]["status"] == "rejected"
            && names_of(&human_draft, "generated").contains("fk_ship"),
        format!("{:?}", names_of(&human_draft, "effective")),
    );
    reject_column(
        &mut human,
        &first,
        "column/public.orders/customer_id",
        "public.orders",
        "customer_id",
    );
    let human_and_column = compile(human);
    guard(failures, "human-reject-plus-other-column", &human_and_column);
    record(
        failures,
        "human-reject-plus-other-column/no-spurious-on-ship",
        queue_item_opt(&human_and_column, item, "relationship_endpoint_not_effective").is_none()
            && closure_detail(&human_and_column, "relationship/public.orders/fk_bill") == "from_column_not_effective"
            && names_of(&human_and_column, "effective").is_empty(),
        format!(
            "ship_endpoint={} left={:?}",
            closure_detail(&human_and_column, item),
            names_of(&human_and_column, "effective")
        ),
    );

    let mut dataset_reject = envelope(probe_orders());
    let first = compile(dataset_reject.clone());
    push_review(
        &mut dataset_reject,
        json!({
            "item_id": "dataset/public.customer",
            "action": "reject",
            "schema_hash": first["provenance"]["schema_hash"],
            "fact_fingerprint": dataset_fingerprint("public", "customer"),
        }),
    );
    dataset_reject["curated"] = json!({
        "datasets": [{"dataset": "public.customer"}],
        "relationships": []
    });
    let code = compile_structure_draft(&bind(dataset_reject)).map(|_| "ok".to_string()).unwrap_err().code;
    record(
        failures,
        "dataset-reject-action/not-in-scope",
        code.ends_with("/review.not_in_scope"),
        code,
    );

    let raw_schema = "A".repeat(31);
    let mut parent = table(
        "parent",
        vec![int4(1, "id", true, None)],
        vec![key_col(1, "id")],
        Some(false),
        Vec::new(),
        Vec::new(),
    );
    parent["schema"] = json!(raw_schema);
    parent["schema_ident"] = json!(ident(&raw_schema));
    let child = table(
        "child",
        vec![int4(1, "id", true, None), int4(2, "parent_id", true, None)],
        vec![key_col(1, "id")],
        Some(false),
        Vec::new(),
        vec![json!({
            "name": "child_parent_fkey",
            "columns": [{
                "attnum": 2,
                "name": "parent_id",
                "ref_schema": raw_schema,
                "ref_table": "parent",
                "ref_attnum": 1,
                "ref_name": "id"
            }]
        })],
    );
    let hidden = compile(envelope(vec![parent, child]));
    guard(failures, "dataset-not-emitted", &hidden);
    let hidden_names = names_of(&hidden, "effective");
    record(
        failures,
        "dataset-not-emitted/no-dangling-fk",
        hidden_names.is_empty()
            && hidden["effective"]["datasets"]
                .as_array()
                .unwrap()
                .iter()
                .all(|dataset| dataset["dataset"] == "public.child")
            && hidden["review_queue"].as_array().unwrap().iter().any(|item| {
                item["kind"] == "identifier_not_in_oos"
            }),
        format!("{hidden_names:?}"),
    );

    let mut unverified = envelope(probe_orders());
    unverified["catalog"]["tables"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|table| table["name"] == "orders")
        .unwrap()["foreign_keys"]
        .as_array_mut()
        .unwrap()
        .push(fk_one("fk_name", 3, "ship_to", "customer", 2, "name"));
    refresh_selection(&mut unverified);
    let unverified_draft = compile(unverified);
    guard(failures, "unverified-key", &unverified_draft);
    let unverified_rel = relationship_named(&unverified_draft, "effective", "fk_name").unwrap();
    record(
        failures,
        "unverified-key/stays-effective",
        unverified_rel["status"] == "draft"
            && unverified_rel["to"]["columns"] == json!(["name"])
            && queue_item(
                &unverified_draft,
                "relationship/public.orders/fk_name",
                "fk_target_key_unverified",
            )["state"]
                == "pending"
            && queue_item(
                &unverified_draft,
                "relationship/public.orders/fk_name",
                "fk_target_key_unverified",
            )["publishable"]
                == false
            && names_of(&unverified_draft, "effective").len() == 4,
        "unverified foreign key was dropped or published",
    );

    let account = table(
        "account",
        vec![
            int4(1, "tenant_id", true, None),
            int4(2, "id", true, None),
            int4(3, "extra", false, None),
        ],
        vec![key_col(2, "id"), key_col(1, "tenant_id")],
        Some(false),
        Vec::new(),
        Vec::new(),
    );
    let invoice = table(
        "invoice",
        vec![
            int4(1, "id", true, None),
            int4(2, "tenant_id", true, None),
            int4(3, "account_id", true, None),
            int4(4, "warehouse_id", true, None),
        ],
        vec![key_col(1, "id")],
        Some(false),
        Vec::new(),
        vec![
            json!({
                "name": "invoice_account_fkey",
                "columns": [
                    {"attnum": 3, "name": "account_id", "ref_schema": "public", "ref_table": "account", "ref_attnum": 2, "ref_name": "id"},
                    {"attnum": 2, "name": "tenant_id", "ref_schema": "public", "ref_table": "account", "ref_attnum": 1, "ref_name": "tenant_id"}
                ]
            }),
            fk_one("invoice_warehouse_fkey", 4, "warehouse_id", "warehouse", 1, "id"),
        ],
    );
    let warehouse = table(
        "warehouse",
        vec![int4(1, "id", true, None)],
        vec![key_col(1, "id")],
        Some(false),
        Vec::new(),
        Vec::new(),
    );
    let composite_tables = vec![account, invoice, warehouse];
    let composite_open = compile(envelope(composite_tables.clone()));
    guard(failures, "composite-open", &composite_open);
    record(
        failures,
        "composite-order",
        end_columns(&composite_open, "generated", "invoice_account_fkey", "from") == vec!["account_id".to_string(), "tenant_id".to_string()]
            && end_columns(&composite_open, "generated", "invoice_account_fkey", "to") == vec!["id".to_string(), "tenant_id".to_string()]
            && dataset(&composite_open, "effective", "public.account")["primary_key"] == json!(["id", "tenant_id"]),
        "composite or primary-key order changed",
    );
    let mut partial = envelope(composite_tables);
    let first = compile(partial.clone());
    reject_column(
        &mut partial,
        &first,
        "column/public.account/tenant_id",
        "public.account",
        "tenant_id",
    );
    let partial_draft = compile(partial);
    guard(failures, "partial-composite", &partial_draft);
    record(
        failures,
        "partial-composite/whole-fk",
        relationship_named(&partial_draft, "effective", "invoice_account_fkey").is_none()
            && closure_detail(&partial_draft, "relationship/public.invoice/invoice_account_fkey") == "to_column_not_effective"
            && names_of(&partial_draft, "effective") == BTreeSet::from(["invoice_warehouse_fkey".to_string()])
            && column_of(dataset(&partial_draft, "effective", "public.invoice"), "account_id").is_some()
            && end_columns(&partial_draft, "generated", "invoice_account_fkey", "from") == vec!["account_id".to_string(), "tenant_id".to_string()],
        closure_detail(&partial_draft, "relationship/public.invoice/invoice_account_fkey"),
    );

    let from_self = compile(fresh_reject(
        employee_tables(),
        &[("column/public.employee/manager_id", "public.employee", "manager_id")],
    ));
    guard(failures, "self-from", &from_self);
    record(
        failures,
        "self-from",
        closure_detail(&from_self, "relationship/public.employee/employee_manager_fkey") == "from_column_not_effective"
            && names_of(&from_self, "effective").is_empty()
            && dataset(&from_self, "effective", "public.employee")["primary_key"] == json!(["id"]),
        closure_detail(&from_self, "relationship/public.employee/employee_manager_fkey"),
    );
    let to_self = compile(fresh_reject(
        employee_tables(),
        &[("column/public.employee/id", "public.employee", "id")],
    ));
    guard(failures, "self-to", &to_self);
    record(
        failures,
        "self-to",
        closure_detail(&to_self, "relationship/public.employee/employee_manager_fkey") == "to_column_not_effective"
            && column_of(dataset(&to_self, "effective", "public.employee"), "manager_id").is_some(),
        closure_detail(&to_self, "relationship/public.employee/employee_manager_fkey"),
    );

    let accent = "é";
    let accent_ident = ident(accent);
    let parent = table(
        "parent",
        vec![int4(1, accent, true, None)],
        vec![key_col(1, accent)],
        Some(false),
        Vec::new(),
        Vec::new(),
    );
    let child = table(
        "child",
        vec![int4(1, "id", true, None), int4(2, "parent_ref", true, None)],
        vec![key_col(1, "id")],
        Some(false),
        Vec::new(),
        vec![json!({
            "name": "child_parent_fkey",
            "columns": [{
                "attnum": 2,
                "name": "parent_ref",
                "ref_schema": "public",
                "ref_table": "parent",
                "ref_attnum": 1,
                "ref_name": accent
            }]
        })],
    );
    let mut nfc_root = envelope(vec![parent, child]);
    let nfc_first = compile(nfc_root.clone());
    let nfc_item = format!("column/public.parent/{accent_ident}");
    reject_column(&mut nfc_root, &nfc_first, &nfc_item, "public.parent", &accent_ident);
    let nfc_draft = compile(nfc_root);
    guard(failures, "nfc-endpoint", &nfc_draft);
    record(
        failures,
        "nfc-endpoint/canonical-reject",
        closure_detail(&nfc_draft, "relationship/public.child/child_parent_fkey") == "to_column_not_effective"
            && column_of(dataset(&nfc_draft, "effective", "public.parent"), &accent_ident).is_none(),
        closure_detail(&nfc_draft, "relationship/public.child/child_parent_fkey"),
    );
    let mut nfd_root = envelope(probe_orders());
    let nfd_first = compile(nfd_root.clone());
    push_review(
        &mut nfd_root,
        json!({
            "item_id": format!("column/public.orders/{}", ident("e\u{0301}")),
            "action": "reject",
            "schema_hash": nfd_first["provenance"]["schema_hash"],
            "fact_fingerprint": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        }),
    );
    let nfd_code = compile_structure_draft(&bind(nfd_root)).map(|_| "ok".to_string()).unwrap_err().code;
    record(
        failures,
        "nfc-endpoint/noncanonical-item-rejected",
        nfd_code.ends_with("/review.unknown_item") || nfd_code.ends_with("/stale_decision") || nfd_code.ends_with("/field.invalid"),
        nfd_code,
    );
}

fn duplicate_cases(failures: &mut Vec<String>) {
    let absent = [CANARY_NAME, CANARY_COMMENT, "leak_pk_9f3a", "DO_NOT_SURFACE"];
    let columns = vec![
        int4(1, CANARY_NAME, true, Some(CANARY_COMMENT)),
        int4(2, "other", false, None),
    ];
    let mut owned = table(
        "customer",
        columns.clone(),
        vec![key_col(1, CANARY_NAME), key_col(1, CANARY_NAME)],
        Some(false),
        Vec::new(),
        Vec::new(),
    );
    owned["owner"] = json!(CANARY_COMMENT);
    owned["comment"] = evidence(CANARY_COMMENT);
    let exact = bind(envelope(vec![owned]));
    write_envelope("duplicate-exact.json", &exact);
    record(
        failures,
        "duplicate/exact-attnum-name",
        static_error(&exact, "/duplicate.key_member", &absent).is_ok(),
        static_error(&exact, "/duplicate.key_member", &absent).err().unwrap_or_default(),
    );

    let attnum_alias = bind(envelope(vec![table(
        "customer",
        columns.clone(),
        vec![key_col(1, CANARY_NAME), key_col(1, "other")],
        Some(false),
        Vec::new(),
        Vec::new(),
    )]));
    record(
        failures,
        "duplicate/same-attnum-other-name",
        static_error(&attnum_alias, "/duplicate.key_member", &absent).is_ok(),
        static_error(&attnum_alias, "/duplicate.key_member", &absent).err().unwrap_or_default(),
    );
    let name_alias = bind(envelope(vec![table(
        "customer",
        columns.clone(),
        vec![key_col(1, CANARY_NAME), key_col(9, CANARY_NAME)],
        Some(false),
        Vec::new(),
        Vec::new(),
    )]));
    record(
        failures,
        "duplicate/same-name-other-attnum",
        static_error(&name_alias, "/duplicate.key_member", &absent).is_ok(),
        static_error(&name_alias, "/duplicate.key_member", &absent).err().unwrap_or_default(),
    );

    let accent = int4(1, "é", true, None);
    let nfc = bind(envelope(vec![table(
        "customer",
        vec![accent, int4(2, "other", false, None)],
        vec![key_col(1, "é"), key_col(2, "e\u{0301}")],
        Some(false),
        Vec::new(),
        Vec::new(),
    )]));
    record(
        failures,
        "duplicate/pk-nfc-alias",
        static_error(&nfc, "/duplicate.key_member", &["é", "\u{0301}", CANARY_COMMENT]).is_ok(),
        static_error(&nfc, "/duplicate.key_member", &["é", "\u{0301}"]).err().unwrap_or_default(),
    );
    let unique = bind(envelope(vec![table(
        "customer",
        columns.clone(),
        vec![key_col(1, CANARY_NAME)],
        Some(false),
        vec![json!({
            "name": "customer_other_key",
            "columns": [key_col(2, "other"), key_col(2, "other")],
            "nulls_not_distinct": false
        })],
        Vec::new(),
    )]));
    record(
        failures,
        "duplicate/unique",
        static_error(&unique, "/duplicate.key_member", &absent).is_ok(),
        static_error(&unique, "/duplicate.key_member", &absent).err().unwrap_or_default(),
    );

    let child_columns = vec![
        int4(1, "id", true, None),
        int4(2, CANARY_NAME, true, None),
        int4(3, "other", false, None),
    ];
    let fk_source = bind(envelope(vec![table(
        "child",
        child_columns.clone(),
        vec![key_col(1, "id")],
        Some(false),
        Vec::new(),
        vec![json!({
            "name": "child_fkey",
            "columns": [
                {"attnum": 2, "name": CANARY_NAME, "ref_schema": "public", "ref_table": "customer", "ref_attnum": 1, "ref_name": "id"},
                {"attnum": 2, "name": CANARY_NAME, "ref_schema": "public", "ref_table": "customer", "ref_attnum": 2, "ref_name": "other"}
            ]
        })],
    )]));
    record(
        failures,
        "duplicate/fk-source",
        static_error(&fk_source, "/duplicate.key_member", &absent).is_ok(),
        static_error(&fk_source, "/duplicate.key_member", &absent).err().unwrap_or_default(),
    );
    let fk_ref = bind(envelope(vec![table(
        "child",
        child_columns.clone(),
        vec![key_col(1, "id")],
        Some(false),
        Vec::new(),
        vec![json!({
            "name": "child_fkey",
            "columns": [
                {"attnum": 2, "name": CANARY_NAME, "ref_schema": "public", "ref_table": "customer", "ref_attnum": 1, "ref_name": "id"},
                {"attnum": 3, "name": "other", "ref_schema": "public", "ref_table": "customer", "ref_attnum": 1, "ref_name": "other"}
            ]
        })],
    )]));
    record(
        failures,
        "duplicate/fk-referenced-attnum",
        static_error(&fk_ref, "/duplicate.key_member", &absent).is_ok(),
        static_error(&fk_ref, "/duplicate.key_member", &absent).err().unwrap_or_default(),
    );
    let fk_nfc = bind(envelope(vec![table(
        "child",
        vec![int4(1, "id", true, None), int4(2, "left_ref", true, None), int4(3, "right_ref", true, None)],
        vec![key_col(1, "id")],
        Some(false),
        Vec::new(),
        vec![json!({
            "name": "child_fkey",
            "columns": [
                {"attnum": 2, "name": "left_ref", "ref_schema": "public", "ref_table": "customer", "ref_attnum": 1, "ref_name": "é"},
                {"attnum": 3, "name": "right_ref", "ref_schema": "public", "ref_table": "customer", "ref_attnum": 2, "ref_name": "e\u{0301}"}
            ]
        })],
    )]));
    record(
        failures,
        "duplicate/fk-referenced-nfc",
        static_error(&fk_nfc, "/duplicate.key_member", &["é", "\u{0301}", CANARY_NAME]).is_ok(),
        static_error(&fk_nfc, "/duplicate.key_member", &["é"]).err().unwrap_or_default(),
    );

    let empty_unique = serde_json::to_string(&envelope(vec![table(
        "customer",
        vec![int4(1, "id", true, None)],
        vec![key_col(1, "id")],
        Some(false),
        vec![json!({"name": "empty_unique", "columns": [], "nulls_not_distinct": false})],
        Vec::new(),
    )])).unwrap();
    record(
        failures,
        "duplicate/empty-unique-is-key-empty",
        static_error(&empty_unique, "/key.empty", &absent).is_ok(),
        static_error(&empty_unique, "/key.empty", &absent).err().unwrap_or_default(),
    );
    let empty_fk = serde_json::to_string(&envelope(vec![table(
        "customer",
        vec![int4(1, "id", true, None)],
        vec![key_col(1, "id")],
        Some(false),
        Vec::new(),
        vec![json!({"name": "empty_fkey", "columns": []})],
    )])).unwrap();
    record(
        failures,
        "duplicate/empty-fk-is-key-empty",
        static_error(&empty_fk, "/key.empty", &absent).is_ok(),
        static_error(&empty_fk, "/key.empty", &absent).err().unwrap_or_default(),
    );
    let illegal = serde_json::to_string(&envelope(vec![table(
        "customer",
        vec![int4(1, "id", true, None)],
        vec![key_col(0, "missing"), key_col(1601, "also_missing")],
        Some(false),
        Vec::new(),
        Vec::new(),
    )])).unwrap();
    record(
        failures,
        "duplicate/illegal-attnum-still-rejected",
        static_error(&illegal, "/key.unknown_column", &["missing", "also_missing"]).is_ok(),
        static_error(&illegal, "/key.unknown_column", &["missing"]).err().unwrap_or_default(),
    );
    let empty_pk = compile(envelope(vec![table(
        "customer",
        vec![int4(1, "id", true, None), int4(2, "other", false, None)],
        Vec::new(),
        None,
        vec![json!({
            "name": "customer_other_key",
            "columns": [key_col(2, "other"), key_col(1, "id")],
            "nulls_not_distinct": false
        })],
        Vec::new(),
    )]));
    guard(failures, "empty-pk-legal-unique", &empty_pk);
    record(
        failures,
        "duplicate/empty-pk-does-not-reject-distinct-unique",
        empty_pk["effective"]["datasets"][0]["primary_key"].as_array().unwrap().is_empty(),
        "empty primary key was rejected or filled",
    );

    let legal = compile(envelope(vec![table(
        "customer",
        vec![
            int4(1, "id", true, None),
            int4(2, "other", false, None),
            int4(3, "Id", true, None),
        ],
        vec![key_col(2, "other"), key_col(1, "id")],
        Some(false),
        vec![json!({
            "name": "customer_id_key",
            "columns": [key_col(1, "id")],
            "nulls_not_distinct": false
        })],
        vec![
            fk_one("customer_self_a", 1, "id", "customer", 3, "Id"),
            fk_one("customer_self_b", 1, "id", "customer", 3, "Id"),
        ],
    )]));
    guard(failures, "distinct-keys", &legal);
    record(
        failures,
        "duplicate/distinct-keys-and-case-stay",
        dataset(&legal, "effective", "public.customer")["primary_key"] == json!(["other", "id"])
            && names_of(&legal, "effective").len() == 2
            && end_columns(&legal, "effective", "customer_self_a", "from") == vec!["id".to_string()]
            && end_columns(&legal, "effective", "customer_self_a", "to") == vec![ident("Id")],
        "distinct keys, case-distinct columns, or shared foreign keys were over-rejected",
    );

    let mut wrong = serde_json::from_str::<Value>(&exact).unwrap();
    wrong["catalog"]["schema_hash"] = json!(ZERO);
    let err = compile_structure_draft(&serde_json::to_string(&wrong).unwrap()).unwrap_err();
    record(
        failures,
        "duplicate/wrong-catalog-hash-first",
        err.code.ends_with("/hash.schema") && !err.to_string().contains(CANARY_NAME),
        err.to_string(),
    );
    let mut wrong_binding = serde_json::from_str::<Value>(&exact).unwrap();
    wrong_binding["bindings"]["schema_hash"] = json!(ZERO);
    let err = compile_structure_draft(&serde_json::to_string(&wrong_binding).unwrap()).unwrap_err();
    record(
        failures,
        "duplicate/wrong-binding-schema-first",
        err.code.ends_with("/hash.schema") && !err.to_string().contains(CANARY_NAME),
        err.to_string(),
    );
    for (field, suffix) in [
        ("config_hash", "/hash.config"),
        ("type_hash", "/hash.type"),
        ("evidence_hash", "/hash.evidence"),
    ] {
        let mut wrong_one = serde_json::from_str::<Value>(&exact).unwrap();
        wrong_one["bindings"][field] = json!(ZERO);
        let err = compile_structure_draft(&serde_json::to_string(&wrong_one).unwrap()).unwrap_err();
        record(
            failures,
            &format!("duplicate/wrong-{field}-first"),
            err.code.ends_with(suffix)
                && !err.to_string().contains(CANARY_NAME)
                && !err.to_string().contains(CANARY_COMMENT),
            err.to_string(),
        );
    }

    let decomposed = "\u{0045}\u{0304}\u{0300}";
    let composed = "\u{1E14}";
    let composed_ident = ident(composed);
    let decomposed_ident = ident(decomposed);
    let same_ident = composed_ident == decomposed_ident;
    note(
        "nfc-u1e14/idents",
        true,
        &format!("same={same_ident} composed={composed_ident} decomposed={decomposed_ident}"),
    );
    let two_columns = compile_structure_draft(&serde_json::to_string(&envelope(vec![table(
        "customer",
        vec![int4(1, composed, true, None), int4(2, decomposed, false, None)],
        vec![key_col(1, composed)],
        Some(false),
        Vec::new(),
        Vec::new(),
    )])).unwrap());
    let two_code = match &two_columns {
        Ok(_) => "compiled".to_string(),
        Err(err) => err.code.clone(),
    };
    record(
        failures,
        "nfc-u1e14/two-columns-not-a-new-duplicate-hole",
        if same_ident {
            two_code.ends_with("/ident.collision")
        } else {
            two_code == "compiled" || two_code.ends_with("/ident.collision")
        },
        format!("same_ident={same_ident} code={two_code}"),
    );
    let repeated_mark = compile_structure_draft(&bind(envelope(vec![table(
        "customer",
        vec![int4(1, composed, true, None), int4(2, "other", false, None)],
        vec![key_col(1, composed), key_col(2, decomposed)],
        Some(false),
        Vec::new(),
        Vec::new(),
    )])));
    let repeated_code = match &repeated_mark {
        Ok(_) => "compiled".to_string(),
        Err(err) => err.code.clone(),
    };
    record(
        failures,
        "nfc-u1e14/key-members-rejected",
        repeated_code.ends_with("/duplicate.key_member") || repeated_code.ends_with("/key.unknown_column") || repeated_code.ends_with("/ident.collision"),
        format!("same_ident={same_ident} code={repeated_code}"),
    );
    record(
        failures,
        "nfc-u1e14/not-accepted",
        repeated_code != "compiled" && two_code != "compiled" || !same_ident && two_code == "compiled",
        format!("two={two_code} key={repeated_code} same={same_ident}"),
    );

    let shared = compile(envelope(probe_orders()));
    write_envelope("shared-baseline.json", &bind(envelope(probe_orders())));
    record(
        failures,
        "duplicate/shared-fk-columns-legal",
        names_of(&shared, "effective").len() == 3,
        "two foreign keys sharing customer_id were rejected",
    );
}

#[test]
fn a086_independent_counterexamples() {
    let dir = evidence_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::remove_file(dir.join("cases.jsonl"));
    let mut failures = Vec::new();
    check_fixtures(&mut failures);
    closure_cases(&mut failures);
    duplicate_cases(&mut failures);
    let summary = json!({
        "failures": failures.len(),
        "ok": failures.is_empty(),
        "items": failures,
    });
    std::fs::write(dir.join("summary.json"), serde_json::to_string_pretty(&summary).unwrap()).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
