use serde_json::{Value, json};
use sqlx::{Connection, Row};
use std::io::{Read, Write};

fn digest(value: &Value) -> String {
    crate::target_installer::sha256_hex(serde_json::to_vec(value).expect("JSON value").as_slice())
}

fn dataset_binding(dataset:String,source:&str,generation:&Value,identity:&Value)->Result<Value,String> {
    for key in ["config_digest","type_digest","input_digest"] {
        if identity[key].as_str().is_none_or(|value|value.len()!=64
            || !value.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))) {
            return Err(format!("published dataset {key} absent or invalid"));
        }
    }
    Ok(json!({"dataset":dataset,"source":source,"generation":generation,
        "config_digest":identity["config_digest"],"type_digest":identity["type_digest"],
        "input_digest":identity["input_digest"]}))
}

fn table_grade(run: &Value, table: &Value, watermark: &Value, published: Option<&Value>) -> Result<Value, String> {
    if run["state"] != "published" { return Ok(watermark["consistency_grade"].clone()); }
    let receipt = published.ok_or("published table receipt absent")?;
    if receipt["source_id"] != run["source_id"]
        || receipt["source_generation"] != run["source_generation"]
        || receipt["target_generation"] != run["target_generation"] {
        return Err("published table receipt identity differs".into());
    }
    let matches = receipt["table_vector"].as_array().ok_or("published table vector")?
        .iter().filter(|e| e["run_id"] == run["run_id"] && e["schema"] == table["schema_ident"]
            && e["table"] == table["table_ident"] && e["physical_name"] == table["physical_name"])
        .collect::<Vec<_>>();
    if matches.len() != 1 || !matches!(matches[0]["consistency_grade"].as_str(), Some("S0" | "B")) {
        return Err("published table grade proof differs".into());
    }
    // A new source-wide B cut must not rewrite an unchanged table's sealed S0 proof.
    Ok(matches[0]["consistency_grade"].clone())
}

pub async fn prepare(document: &Value) -> Result<Value, String> {
    let object = document.as_object().ok_or("request")?;
    if object.len() != 3
        || document["protocol"] != "oo.pipeline.native-library-prepare/v1"
        || !object.contains_key("target")
        || !object.contains_key("manifest")
    {
        return Err("closed request".into());
    }
    let manifest = &document["manifest"];
    let id = manifest["library_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("library_id")?;
    let sources = manifest["sources"]
        .as_array()
        .filter(|s| !s.is_empty())
        .ok_or("sources")?;
    let mut db = crate::native_update::connect_target(&document["target"])
        .await
        .map_err(str::to_string)?;
    let mut tx = db.begin().await.map_err(|e| e.to_string())?;
    let mut shards = Vec::new();
    let mut datasets = Vec::new();
    let mut members = Vec::new();
    let mut sorted = sources.iter().collect::<Vec<_>>();
    sorted.sort_by_key(|s| s["source_id"].as_str().unwrap_or(""));
    for source in sorted {
        let name = source["source_id"].as_str().ok_or("source_id")?;
        let schema = source["schema"].as_str().ok_or("schema")?;
        let names = source["tables"].as_array().ok_or("tables")?;
        let row = sqlx::query(
            "SELECT to_jsonb(w)::text FROM _oo.source_watermark w WHERE source=$1 FOR UPDATE",
        )
        .bind(name)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        let watermark: Value =
            serde_json::from_str(row.get::<&str, _>(0)).map_err(|e| e.to_string())?;
        if watermark["consistency"] == "transactional" {
            // The database validates the original C1 receipt against its durable
            // target ACK, control task and complete current storage vector.
            let raw: String = sqlx::query_scalar(
                "SELECT _oo.native_library_c1_shard($1)::text",
            )
            .bind(name)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| format!("C1 library proof: {e}"))?;
            let shard: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            let stored = c1_storage_tables(&shard, schema, names)?;
            for (table, run) in stored {
                let raw: String = sqlx::query_scalar(
                    "SELECT jsonb_build_object('config_digest',config_digest,\
                     'type_digest',type_digest,'input_digest',input_digest)::text \
                     FROM _oo.full_import_run WHERE run_id=$1::uuid AND source_id=$2",
                )
                .bind(run)
                .bind(name)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
                let binding: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
                datasets.push(dataset_binding(format!("{schema}.{table}"),name,
                    &watermark["source_generation"],&binding)?);
            }
            shards.push(shard);
            let mut names = names.clone();
            names.sort_by_key(|n| n.as_str().unwrap_or("").to_owned());
            members.push(json!({"source_id":name,"schema":schema,"tables":names}));
            continue;
        }
        let rows=sqlx::query("SELECT to_jsonb(r)::text,to_jsonb(t)::text,to_jsonb(q)::text,to_jsonb(p)::text FROM _oo.table_freshness f JOIN _oo.full_import_run r ON r.source_id=f.source AND r.source_generation=f.source_generation AND r.target_generation=f.target_generation JOIN _oo.full_import_table t ON t.run_id=r.run_id AND t.physical_name=f.physical_name AND t.schema_ident=f.schema_name AND t.table_ident=f.table_name JOIN _oo.table_quality q ON q.schema_name=f.schema_name AND q.table_name=f.table_name LEFT JOIN _oo.native_publication_receipt p ON p.run_id=r.run_id WHERE f.source=$1 ORDER BY t.schema_ident,t.table_ident FOR UPDATE OF f,r,t")
            .bind(name).fetch_all(&mut *tx).await.map_err(|e|e.to_string())?;
        if rows.len() != names.len() {
            return Err("current source coverage differs".into());
        }
        let mut tables = Vec::new();
        let mut anchor = None;
        for row in rows {
            let run: Value =
                serde_json::from_str(row.get::<&str, _>(0)).map_err(|e| e.to_string())?;
            let table: Value =
                serde_json::from_str(row.get::<&str, _>(1)).map_err(|e| e.to_string())?;
            let quality: Value =
                serde_json::from_str(row.get::<&str, _>(2)).map_err(|e| e.to_string())?;
            let published: Option<Value> = row.get::<Option<&str>, _>(3)
                .map(serde_json::from_str).transpose().map_err(|e| e.to_string())?;
            let grade = table_grade(&run, &table, &watermark, published.as_ref())?;
            if table["schema_ident"] != schema
                || names.iter().filter(|n| **n == table["table_ident"]).count() != 1
            {
                return Err("manifest table coverage differs".into());
            }
            if run["source_generation"] == watermark["source_generation"]
                && run["target_generation"] == watermark["target_generation"]
            {
                anchor = Some(run["run_id"].clone());
            }
            datasets.push(dataset_binding(format!("{}.{}",schema,table["table_ident"].as_str().ok_or("table")?),
                name,&run["source_generation"],&run)?);
            tables.push(json!({"schema":schema,"table":table["table_ident"],"run_id":run["run_id"],"physical_name":table["physical_name"],"accepted_rows":table["applied_rows"],"canonical_digest":table["canonical_digest"],"source_generation":run["source_generation"],"target_generation":run["target_generation"],"applied_pos":table["applied_pos"].as_str().map(str::to_owned).unwrap_or_else(||table["applied_pos"].to_string()),"capture_method":quality["capture_method"],"delete_detection":quality["delete_detection"],"consistency_grade":grade}));
        }
        let anchor = anchor.ok_or("current source anchor absent")?;
        let existing:Option<String>=sqlx::query_scalar("SELECT receipt::text FROM _oo.native_per_table_publication WHERE source_id=$1 AND source_generation=$2 AND target_generation=$3 ORDER BY published_at DESC LIMIT 1")
            .bind(name).bind(watermark["source_generation"].as_i64().ok_or("generation")?).bind(watermark["target_generation"].as_i64().ok_or("generation")?).fetch_optional(&mut *tx).await.map_err(|e|e.to_string())?;
        let receipt = if let Some(text) = existing {
            serde_json::from_str(&text).map_err(|e| e.to_string())?
        } else {
            json!({"protocol":"oo.pipeline.native-publication-receipt/v1","collection_mode":"per_table","run_id":anchor,
                "run_ids":tables.iter().map(|t|t["run_id"].clone()).collect::<Vec<_>>(),"source_id":name,
                "source_generation":watermark["source_generation"],"target_generation":watermark["target_generation"],
                "position_codec":watermark["position_codec"],"position_lineage":watermark["position_lineage"],"applied_pos":"0",
                "consistency":"per_table","consistency_grade":watermark["consistency_grade"],"cross_table_consistent":false,"tables":tables})
        };
        let existing_sha: Option<String> = sqlx::query_scalar(
            "SELECT receipt_sha256 FROM _oo.native_per_table_publication WHERE receipt=$1::jsonb",
        )
        .bind(receipt.to_string())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        let sha = existing_sha.unwrap_or_else(|| digest(&receipt));
        if sha != digest(&receipt) {
            return Err("existing source receipt uses a different canonical digest".into());
        }
        shards.push(
            json!({"source_id":name,"anchor_run":anchor,"receipt_sha256":sha,"receipt":receipt}),
        );
        let mut names = names.clone();
        names.sort_by_key(|n| n.as_str().unwrap_or("").to_owned());
        members.push(json!({"source_id":name,"schema":schema,"tables":names}));
    }
    let membership =
        json!({"protocol":"oo.library.membership/v1","library_id":id,"sources":members});
    let library = json!({"protocol":"oo.pipeline.native-library-publication/v1","library_id":id,"manifest":membership,"manifest_sha256":digest(&membership),"expected_tables":datasets.len(),"shards":shards.iter().map(|s|s["source_id"].clone()).collect::<Vec<_>>()});
    let receipt = json!({"library":library,"shards":shards});
    let sha = digest(&receipt);
    let current_state: Option<String> = sqlx::query_scalar(
        "SELECT state FROM _oo.native_library_head WHERE library_id=$1 AND receipt_sha256=$2",
    )
    .bind(id)
    .bind(&sha)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query("SELECT _oo.publish_native_library($1::jsonb,$2,$3::jsonb)")
        .bind(library.to_string())
        .bind(&sha)
        .bind(receipt["shards"].to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("publication precheck: {e}"))?;
    tx.rollback().await.map_err(|e| e.to_string())?;
    datasets.sort_by_key(|d| d["dataset"].as_str().unwrap_or("").to_owned());
    Ok(
        json!({"protocol":"oo.pipeline.native-library-prepared/v1","library_id":id,"manifest_sha256":digest(manifest),"native_mapping":{"datasets":datasets,"publication":{"protocol":"oo.pipeline.native-library-publication/v1","run_id":receipt["shards"][0]["anchor_run"],"receipt_sha256":sha,"receipt":receipt}},"published":false,"precheck":"rolled_back","current_state":current_state.unwrap_or_else(||"unpublished".into())}),
    )
}

fn c1_storage_tables<'a>(
    shard: &'a Value,
    schema: &str,
    names: &[Value],
) -> Result<Vec<(&'a str, &'a str)>, String> {
    let tables = shard["receipt"]["tables"].as_array().ok_or("C1 table vector")?;
    if tables.len() != names.len() || tables.is_empty() {
        return Err("C1 manifest table coverage differs".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    tables.iter().map(|table| {
        // Storage identities are for lookup only. Never rewrite source names
        // inside the original, hash-bound transactional receipt.
        let namespace = table.get("storage_schema").unwrap_or(&table["schema"]);
        let name = table.get("storage_table").unwrap_or(&table["table"]);
        let name = name.as_str().ok_or("C1 storage table")?;
        let run = table["run_id"].as_str().ok_or("C1 run")?;
        if namespace != schema || names.iter().filter(|n| **n == name).count() != 1
            || !seen.insert(name) {
            return Err("C1 manifest table coverage differs".into());
        }
        Ok((name, run))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn c5_and_c1_bind_the_actual_schema_and_input_digests() {
        let run=json!({"config_digest":"a".repeat(64),"type_digest":"b".repeat(64),
            "input_digest":"c".repeat(64),"source_generation":7});
        let binding=dataset_binding("app.items".into(),"s",&json!(7),&run).unwrap();
        for key in ["config_digest","type_digest","input_digest"] {
            assert_eq!(binding[key],run[key]);
            for bad in [Value::Null,json!(""),json!("G".repeat(64))] {
                let mut mutated=run.clone();mutated[key]=bad;
                assert!(dataset_binding("app.items".into(),"s",&json!(7),&mutated).is_err());
            }
        }
        assert_eq!(binding["generation"],7);
        assert_eq!(binding["dataset"],"app.items");
    }

    #[test]
    fn unchanged_published_table_keeps_its_grade_when_source_cut_changes() {
        let mut run=json!({"state":"published","run_id":"r","source_id":"s","source_generation":1,"target_generation":2});
        let table=json!({"schema_ident":"app","table_ident":"t","physical_name":"pt"});
        let watermark=json!({"consistency_grade":"B"});
        let mut receipt=json!({"source_id":"s","source_generation":1,"target_generation":2,
            "table_vector":[{"run_id":"r","schema":"app","table":"t","physical_name":"pt","consistency_grade":"S0"}]});
        assert_eq!(table_grade(&run,&table,&watermark,Some(&receipt)).unwrap(),"S0");
        receipt["table_vector"][0]["physical_name"]=json!("different");
        assert!(table_grade(&run,&table,&watermark,Some(&receipt)).is_err());
        assert!(table_grade(&run,&table,&watermark,None).is_err());
        run["state"]=json!("awaiting_publish");
        assert_eq!(table_grade(&run,&table,&watermark,None).unwrap(),"B");
    }

    #[test]
    fn c1_manifest_uses_storage_identity_without_rewriting_receipt() {
        let shard = json!({"receipt":{"consistency_grade":"T","tables":[{
            "schema":"SOURCE_OWNER","table":"ORDERS","storage_schema":"q_owner",
            "storage_table":"q_orders","run_id":"run"}]}});
        let original = shard.clone();
        assert_eq!(c1_storage_tables(&shard, "q_owner", &[json!("q_orders")]).unwrap(),
                   vec![("q_orders", "run")]);
        assert_eq!(shard, original);
        assert!(c1_storage_tables(&shard, "SOURCE_OWNER", &[json!("ORDERS")]).is_err());
        assert!(c1_storage_tables(&shard, "q_owner", &[json!("q_orders"), json!("q_orders")]).is_err());
        assert!(c1_storage_tables(&shard, "q_owner", &[json!("foreign")]).is_err());
        let mysql = json!({"receipt":{"tables":[{"schema":"app","table":"items","run_id":"r"}]}});
        assert_eq!(c1_storage_tables(&mysql, "app", &[json!("items")]).unwrap(), vec![("items","r")]);
    }
}

pub async fn stdio() -> Result<(), u8> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(8_388_609)
        .read_to_end(&mut bytes)
        .map_err(|_| 2u8)?;
    if bytes.len() > 8_388_608 {
        return Err(2);
    }
    let document: Value = serde_json::from_slice(&bytes).map_err(|_| 2u8)?;
    let (result, code) = match prepare(&document).await {
        Ok(value) => (value, 0),
        Err(reason) => (
            json!({"protocol":"oo.pipeline.native-library-prepared/v1","published":false,"reason":reason}),
            1,
        ),
    };
    std::io::stdout()
        .write_all(format!("{}\n", result).as_bytes())
        .map_err(|_| 1u8)?;
    if code == 0 { Ok(()) } else { Err(code) }
}
