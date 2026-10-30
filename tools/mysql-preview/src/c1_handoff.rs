//! Metadata-only eligibility and explicit baseline consent for a C5 -> C1 switch.
use super::*;

pub const PROTOCOL: &str = "oo.mysql.c5-to-c1/v1";
pub const BASELINE: &str = "new_snapshot_plus_log_reconciliation";

pub(crate) fn initial_generation(context:&Value)->Result<u64,PreviewError> {
    let fail=||PreviewError::new("handoff","initial_generation");
    let generation=context["source_generation"].as_u64().filter(|n|*n>0).ok_or_else(fail)?;
    let h=&context["handoff"];
    if generation==1 && h.is_null() {return Ok(generation);}
    if h["protocol"]!=PROTOCOL || h["baseline"]!=BASELINE
        || h["source_id"]!=context["source_id"] || h["binding"]!=context["binding"]
        || h["project_id"]!=context["project_id"]
        || h["parent_source_generation"].as_u64().filter(|n|*n>0).and_then(|n|n.checked_add(1))!=Some(generation)
        || h["plan_sha256"].as_str().is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_hexdigit())) {
        return Err(fail());
    }
    Ok(generation)
}

pub(crate) fn eligible(v:&Value)->Result<(),PreviewError> {
    let reason=if v["log_bin"]!="1" {"binlog_off"}
        else if v["gtid_mode"]!="ON" || v["enforce_gtid_consistency"]!="ON" {"gtid_required"}
        else if v["binlog_format"]!="ROW" || v["binlog_row_image"]!="FULL" {"row_full_required"}
        else if v["binlog_expire_logs_seconds"].as_u64().is_none_or(|s|s<RETENTION) {"retention_insufficient"}
        else {return Ok(())};
    Err(PreviewError::new("c1",reason))
}

pub fn validate(config: &Value, context: &Value, uuid: &str) -> Result<(), PreviewError> {
    let Some(h) = config.get("c5_to_c1") else {
        if !context["handoff"].is_null() {
            return Err(PreviewError::new("handoff", "missing_consent"));
        }
        return Ok(());
    };
    if h["protocol"] != PROTOCOL || h["baseline"] != BASELINE
        || h["prior_history"] != "not_recoverable_from_new_logs"
        || h["expected_server_uuid"] != uuid
        || h["source_id"] != context["source_id"]
        || h["binding"] != context["binding"]
        || h["project_id"] != context["project_id"]
        || (!context["handoff"].is_null()
            && (context["handoff"]["source_id"] != h["source_id"]
                || context["handoff"]["baseline"] != BASELINE))
    {
        return Err(PreviewError::new("handoff", "identity_or_consent"));
    }
    Ok(())
}

pub async fn preflight(path: &Path) -> Result<(), PreviewError> {
    let value: Value = serde_json::from_slice(&fs::read(path)
        .map_err(|_| PreviewError::new("config", "path"))?)
        .map_err(|_| PreviewError::new("config", "json"))?;
    let cfg = load_config(path)?;
    configured_selection(&cfg)?;
    let mut conn = connect_cdc(&cfg).await?;
    let facts = preflight_values(&mut conn).await?;
    let uuid = scalar(&mut conn, "SELECT @@GLOBAL.server_uuid").await?;
    let h = &value["c5_to_c1"];
    validate(&value, &json!({"source_id":h["source_id"],"binding":h["binding"],
        "project_id":h["project_id"]}), &uuid)?;
    if h.is_null() {
        return Err(PreviewError::new("handoff", "missing_consent"));
    }
    replication_grants_ok(&rows_first(&conn.query::<Row, _>("SHOW GRANTS FOR CURRENT_USER()")
        .await.map_err(|e| mysql_err(&e))?)?)?;
    let (_, _, catalog) = load_pk(&mut conn, &cfg.tables).await?;
    let (_, _, executed) = full_status(&mut conn).await?;
    let span = parse_span(&executed)?;
    if span.uuid != uuid || span.start != 1 || span.end == 0
        || facts["server_id"].as_str().and_then(|s|s.parse::<u32>().ok()) == Some(cfg.server_id)
    {
        return Err(PreviewError::new("handoff", "position_or_server_id"));
    }
    println!("{}", json!({"protocol":PROTOCOL,"source_id":h["source_id"],"baseline":BASELINE,
        "server_uuid":uuid,"catalog_sha256":sha256_hex(catalog.as_bytes()),
        "source_preflight":facts,"observed_gtid":executed,
        "gtid_purged":scalar(&mut conn,"SELECT @@GLOBAL.gtid_purged").await?,
        "business_rows_read":0,"source_settings_changed":false,
        "prior_c5_is_cdc":false,"historical_events_recoverable":false}));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn consent_binds_identity_and_declares_new_reconciliation() {
        let h=json!({"protocol":PROTOCOL,"source_id":"s","binding":"b","project_id":"p",
            "expected_server_uuid":"u","baseline":BASELINE,"prior_history":"not_recoverable_from_new_logs"});
        let c=json!({"source_id":"s","binding":"b","project_id":"p","handoff":h});
        validate(&json!({"c5_to_c1":h}),&c,"u").unwrap();
        for k in ["source_id","binding","project_id","baseline","prior_history","expected_server_uuid"] {
            let mut bad=h.clone();bad[k]=json!("wrong");
            assert!(validate(&json!({"c5_to_c1":bad}),&c,"u").is_err(),"{k}");
        }
        assert!(validate(&json!({}),&c,"u").is_err());
        validate(&json!({}),&json!({}),"u").unwrap();
    }

    #[test]
    fn log_off_and_short_retention_refuse_without_source_changes() {
        let v=json!({"log_bin":"1","gtid_mode":"ON","enforce_gtid_consistency":"ON",
            "binlog_format":"ROW","binlog_row_image":"FULL","binlog_expire_logs_seconds":604800});
        eligible(&v).unwrap();
        for (key,value,reason) in [
            ("log_bin",json!("0"),"binlog_off"),
            ("gtid_mode",json!("OFF"),"gtid_required"),
            ("enforce_gtid_consistency",json!("OFF"),"gtid_required"),
            ("binlog_format",json!("STATEMENT"),"row_full_required"),
            ("binlog_row_image",json!("MINIMAL"),"row_full_required"),
            ("binlog_expire_logs_seconds",json!(604799),"retention_insufficient"),
            ("binlog_expire_logs_seconds",Value::Null,"retention_insufficient")] {
            let mut bad=v.clone();bad[key]=value;
            assert_eq!(eligible(&bad).unwrap_err().fact,reason);
        }
    }

    #[test]
    fn initial_generation_requires_explicit_authorized_handoff_not_a_reset() {
        assert_eq!(initial_generation(&json!({"source_generation":1})).unwrap(),1);
        assert!(initial_generation(&json!({"source_generation":2})).is_err());
        let mut c=json!({"source_generation":8,"source_id":"s","binding":"b","project_id":"p",
            "handoff":{"protocol":PROTOCOL,"baseline":BASELINE,"source_id":"s","binding":"b","project_id":"p",
                "parent_source_generation":7,"plan_sha256":"a".repeat(64)}});
        assert_eq!(initial_generation(&c).unwrap(),8);
        c["handoff"]["parent_source_generation"]=json!(0);
        assert!(initial_generation(&c).is_err());
        c["handoff"]["parent_source_generation"]=json!(8);
        assert!(initial_generation(&c).is_err());
    }
}
