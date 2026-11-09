//! Pack-bound lookup plans. Execution still goes through the public SQL guard,
//! role session, publication binding and durable query audit.
use super::*;
use crate::pack_catalog::PackMetadata;

fn text_input(body: &[u8], keys: &[&str]) -> Result<Value, &'static str> {
    let value: Value = serde_json::from_slice(body).map_err(|_| "invalid tool request")?;
    let object = value.as_object().ok_or("invalid tool request")?;
    if object.len() != keys.len()
        || keys.iter().any(|key| {
            object.get(*key).and_then(Value::as_str).is_none_or(|s| {
                s.trim().is_empty() || s.len() > 2048 || s.chars().any(char::is_control)
            })
        })
    {
        return Err("invalid tool request");
    }
    Ok(value)
}

fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !s.as_bytes()[0].is_ascii_digit()
}

fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn visible_column(pack: &PackMetadata, table: &str, column: &str) -> bool {
    pack.columns
        .iter()
        .any(|c| c.table == table && c.name == column)
}

fn value_sql(pack: &PackMetadata, path: &str, text: &str) -> Option<String> {
    let (table, column) = path.rsplit_once('.')?;
    if !visible_column(pack, table, column) {
        return None;
    }
    lookup_sql(table, &[column.to_owned()], &[column.to_owned()], text)
}

fn lookup_sql(table: &str, keys: &[String], displays: &[String], text: &str) -> Option<String> {
    let (schema, table) = table.split_once('.')?;
    if !identifier(schema)
        || !identifier(table)
        || keys.is_empty()
        || displays.is_empty()
        || keys.iter().chain(displays).any(|s| !identifier(s))
        || text.len() > 200
        || text.contains(['%', '_', '\\'])
    {
        return None;
    }
    let mut columns = keys.to_vec();
    for display in displays {
        if !columns.contains(display) {
            columns.push(display.clone());
        }
    }
    let columns = columns
        .iter()
        .map(|s| quoted(s))
        .collect::<Vec<_>>()
        .join(", ");
    let pattern = text.replace('\'', "''");
    let predicate = displays
        .iter()
        .map(|s| format!("{} ILIKE '%{pattern}%'", quoted(s)))
        .collect::<Vec<_>>()
        .join(" OR ");
    Some(format!(
        "SELECT {columns} FROM {}.{} WHERE {predicate} GROUP BY {columns} ORDER BY {columns} LIMIT 9",
        quoted(schema),
        quoted(table)
    ))
}

fn entity_field(text: &str, prefix: &str) -> Option<Vec<String>> {
    let line = text.lines().find_map(|line| line.strip_prefix(prefix))?;
    Some(
        line.split('`')
            .enumerate()
            .filter(|(i, _)| i % 2 == 1)
            .map(|(_, s)| s.to_owned())
            .collect(),
    )
}

fn entity_sql(pack: &PackMetadata, text: &str, needle: &str) -> Option<String> {
    let tables = entity_field(text, "primary dataset: ")?;
    let [table] = tables.as_slice() else {
        return None;
    };
    let keys = entity_field(text, "key: ")?;
    let displays = entity_field(text, "display: ")?;
    if keys
        .iter()
        .chain(&displays)
        .any(|c| !visible_column(pack, table, c))
    {
        return None;
    }
    lookup_sql(table, &keys, &displays, needle)
}

pub(super) async fn resolve_entity(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
    peer: crate::http_gateway::PublicPeer,
    AxumPath(entity): AxumPath<String>,
    body: Bytes,
) -> Response {
    lookup(state, headers, raw, peer, Some(entity), body).await
}

pub(super) async fn search_values(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
    peer: crate::http_gateway::PublicPeer,
    body: Bytes,
) -> Response {
    lookup(state, headers, raw, peer, None, body).await
}

async fn lookup(
    state: AppState,
    headers: HeaderMap,
    raw: Option<String>,
    peer: crate::http_gateway::PublicPeer,
    entity: Option<String>,
    body: Bytes,
) -> Response {
    let caller =
        match crate::http_gateway::public_caller(&state, &headers, &raw, peer.clone()).await {
            Ok(caller) => caller,
            Err(response) => return *response,
        };
    let binding = match metadata_binding(&state, &caller, &headers, false).await {
        Ok(binding) => binding,
        Err(response) => return *response,
    };
    let Some(plane) = state.data_plane() else {
        return error_response(
            ErrorCode::AuditUnavailable,
            "lookup audit is unavailable",
            None,
            None,
        );
    };
    let Ok(id) = new_query_id() else {
        return error_response(ErrorCode::Internal, "lookup id unavailable", None, None);
    };
    let audit = WalAudit {
        dir: plane.wal_dir.clone(),
        collector: plane.collector_dir.clone(),
        query_id: id.as_str().to_owned(),
        principal: caller.tuple.principal_id.clone(),
    };
    let fields = serde_json::json!({
        "operation":if entity.is_some() { "resolve_entity" } else { "search_values" },
        "status":"requested", "role":caller.tuple.role, "project":caller.tuple.project_id,
    })
    .as_object()
    .unwrap()
    .clone();
    if audit.persist("lookup", 0, fields).await.is_err() {
        return error_response(
            ErrorCode::AuditUnavailable,
            "lookup audit is unavailable",
            None,
            None,
        );
    }
    let input = match text_input(
        &body,
        if entity.is_some() {
            &["text"]
        } else {
            &["text", "column"]
        },
    ) {
        Ok(input) => input,
        Err(message) => return error_response(ErrorCode::SchemaInvalid, message, None, None),
    };
    let catalog = state.pack_catalog();
    let Some(pack) = find_pack(&catalog, &caller) else {
        return error_response(
            ErrorCode::SchemaInvalid,
            "knowledge pack is not loaded",
            None,
            None,
        );
    };
    let needle = input["text"].as_str().unwrap();
    let mut entity_keys = None;
    let sql = if let Some(name) = &entity {
        if !identifier(name) {
            None
        } else {
            catalog
                .file_text(
                    &pack.project,
                    &pack.role,
                    &pack.policy_version,
                    &pack.knowledge_version,
                    &format!("entities/{name}.md"),
                )
                .and_then(|file| {
                    entity_keys = entity_field(&file.text, "key: ");
                    entity_sql(&pack, &file.text, needle)
                })
        }
    } else {
        value_sql(&pack, input["column"].as_str().unwrap(), needle)
    };
    let Some(sql) = sql else {
        return error_response(
            ErrorCode::SchemaInvalid,
            "lookup is unavailable in this role pack",
            None,
            None,
        );
    };
    let mut headers = headers;
    if let Some((receipt, _, _)) = binding.as_ref() {
        let mut actual = HeaderMap::new();
        receipt.headers(&mut actual);
        if let Some(id) = actual.get(crate::publication_binding::ACTUAL) {
            headers.insert(crate::publication_binding::EXPECTED, id.clone());
        }
    }
    let response = query(
        State(state),
        headers,
        RawQuery(raw),
        peer,
        Bytes::from(
            serde_json::json!({"sql":sql,"max_rows":9,"format":"json",
            "client_context":{"purpose":if entity.is_some() {"resolve"} else {"search"},"turn":0}})
            .to_string(),
        ),
    )
    .await;
    if !response.status().is_success() {
        return response;
    }
    let (parts, body) = response.into_parts();
    let bytes = match axum::body::to_bytes(body, 4 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => return error_response(ErrorCode::Internal, "lookup unavailable", None, None),
    };
    let mut result: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return error_response(ErrorCode::Internal, "lookup unavailable", None, None),
    };
    let count = result["rows"].as_array().map_or(0, Vec::len);
    let more = count > 8 || result["truncated"] == true;
    if let Some(rows) = result["rows"].as_array_mut() {
        rows.truncate(8);
    }
    result["returned_rows"] = Value::from(count.min(8));
    result["truncated"] = Value::Bool(more);
    result["lookup_status"] = Value::from(if count == 0 {
        "not_found"
    } else if entity.is_some() && count == 1 && !more {
        "resolved"
    } else if entity.is_some() {
        "ambiguous"
    } else {
        "matches"
    });
    result["requires_confirmation"] = Value::Bool(entity.is_some() && (count > 1 || more));
    result["method"] = Value::from("contains");
    result["lookup_audit_id"] = Value::from(id.as_str());
    if let Some(keys) = entity_keys {
        // SQL projects the ordered key columns first, including composite keys.
        result["candidates"] =
            result["rows"]
                .as_array()
                .map(|rows| {
                    Value::Array(rows.iter().map(|row| {
                let key: Map<String, Value> = keys.iter().enumerate()
                    .map(|(i, name)| (name.clone(), row[i].clone())).collect();
                serde_json::json!({"entity_key":key, "row":row, "method":"contains"})
            }).collect())
                })
                .unwrap_or_else(|| Value::Array(vec![]));
    }
    let mut response = json_ok(result);
    response.headers_mut().extend(parts.headers);
    response.headers_mut().remove(header::CONTENT_LENGTH);
    response
}

pub(super) async fn report_gap(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
    peer: crate::http_gateway::PublicPeer,
    body: Bytes,
) -> Response {
    let caller = match crate::http_gateway::public_caller(&state, &headers, &raw, peer).await {
        Ok(caller) => caller,
        Err(response) => return *response,
    };
    let binding = match metadata_binding(&state, &caller, &headers, false).await {
        Ok(binding) => binding,
        Err(response) => return *response,
    };
    let input = match text_input(&body, &["description"]) {
        Ok(input) => input,
        Err(message) => return error_response(ErrorCode::SchemaInvalid, message, None, None),
    };
    let Some(plane) = state.data_plane() else {
        return error_response(
            ErrorCode::AuditUnavailable,
            "gap storage is unavailable",
            None,
            None,
        );
    };
    if find_pack(&state.pack_catalog(), &caller).is_none() || plane.role_for(&caller).is_none() {
        return error_response(
            ErrorCode::SchemaInvalid,
            "role is not published",
            None,
            None,
        );
    }
    let Ok(id) = new_query_id() else {
        return error_response(ErrorCode::Internal, "gap id unavailable", None, None);
    };
    // Keep untrusted feedback in a private inbox, not the audit collector.
    // An inbox entry without its durable audit row is not accepted work.
    let payload = serde_json::json!({"description":input["description"], "untrusted":true,
        "role":caller.tuple.role, "project":caller.tuple.project_id,
        "knowledge_version":caller.tuple.knowledge_version,
        "policy_version":caller.tuple.policy_version})
    .to_string();
    if persist_gap(&plane.wal_dir, id.as_str(), &payload).is_err() {
        return error_response(
            ErrorCode::AuditUnavailable,
            "gap storage is unavailable",
            None,
            None,
        );
    }
    use sha2::{Digest, Sha256};
    let fields = serde_json::json!({
        "operation":"report_gap", "status":"pending_review", "untrusted":true,
        "payload_sha256":format!("{:x}", Sha256::digest(payload.as_bytes())), "role":caller.tuple.role,
        "project":caller.tuple.project_id, "policy_version":caller.tuple.policy_version,
        "knowledge_version":caller.tuple.knowledge_version,
    })
    .as_object()
    .unwrap()
    .clone();
    let audit = WalAudit {
        dir: plane.wal_dir.clone(),
        collector: plane.collector_dir.clone(),
        query_id: id.as_str().to_owned(),
        principal: caller.tuple.principal_id.clone(),
    };
    if audit.persist("gap", 0, fields).await.is_err() {
        return error_response(
            ErrorCode::AuditUnavailable,
            "gap was not durably recorded",
            None,
            None,
        );
    }
    with_binding(
        json_ok(
            serde_json::json!({"gap_id":id.as_str(), "audit_id":id.as_str(),
        "status":"pending_review"}),
        ),
        binding.as_ref().map(|(receipt, _, _)| receipt),
    )
}

fn persist_gap(dir: &Path, id: &str, payload: &str) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(dir.join(format!("{id}.gap")))?;
    file.write_all(payload.as_bytes())?;
    file.sync_all()?;
    File::open(dir)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> PackMetadata {
        PackMetadata {
            project: "p".into(),
            role: "r".into(),
            policy_version: "1".into(),
            knowledge_version: "1".into(),
            pack_digest: "d".into(),
            files: vec![],
            columns: ["id", "name"]
                .iter()
                .map(|name| crate::pack_catalog::PackColumn {
                    table: "app.customers".into(),
                    name: (*name).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn lookup_uses_only_published_columns_and_quotes_literals() {
        let pack = pack();
        assert!(value_sql(&pack, "app.customers.phone", "a").is_none());
        assert!(value_sql(&pack, "other.customers.name", "a").is_none());
        let sql = value_sql(&pack, "app.customers.name", "O'Reilly").unwrap();
        assert!(sql.contains("ILIKE '%O''Reilly%'"));
        assert!(sql.ends_with("LIMIT 9"));
        for text in ["%", "_", "\\"] {
            assert!(value_sql(&pack, "app.customers.name", text).is_none());
        }
        let entity = "primary dataset: `app.customers`\nkey: `id`\ndisplay: `name`\n";
        assert!(entity_sql(&pack, entity, "Alice").is_some());
        assert!(entity_sql(&pack, &entity.replace("`name`", "`phone`"), "Alice").is_none());
        assert!(entity_sql(&pack, "primary dataset: `other.customers`", "Alice").is_none());
    }

    #[test]
    fn tool_input_is_closed_and_bounded() {
        for body in [
            r#"{"text":""}"#,
            r#"{"text":"x","role":"admin"}"#,
            r#"{"text":"x\n"}"#,
            r#"[]"#,
        ] {
            assert!(text_input(body.as_bytes(), &["text"]).is_err());
        }
        assert!(text_input(br#"{"description":"Missing metric"}"#, &["description"]).is_ok());
    }

    #[tokio::test]
    async fn private_gap_and_audit_are_durable_and_do_not_overwrite() {
        use std::os::unix::fs::PermissionsExt;
        let root =
            std::env::temp_dir().join(format!("oo-gap-{}", new_query_id().unwrap().as_str()));
        fs::create_dir(&root).unwrap();
        persist_gap(&root, "q_gap", "private-canary").unwrap();
        assert_eq!(
            fs::metadata(root.join("q_gap.gap"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(persist_gap(&root, "q_gap", "replacement").is_err());
        let audit = WalAudit {
            dir: root.clone(),
            collector: None,
            query_id: "q_gap".into(),
            principal: "fixture".into(),
        };
        audit
            .persist(
                "gap",
                0,
                serde_json::json!({"operation":"report_gap","status":"pending_review"})
                    .as_object()
                    .unwrap()
                    .clone(),
            )
            .await
            .unwrap();
        let line = fs::read_to_string(root.join("q_gap-gap.wal")).unwrap();
        assert!(!line.contains("private-canary"));
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap()["phase"],
            "gap"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
