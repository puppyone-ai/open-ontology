//! ACK for the opt-in typed C1 staging slice. A target commit is not an ACK.
//! This checkpoint does not mark a jobs task succeeded or publish a dataset.
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_postgres::Client;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    protocol: String,
    source: String,
    task: String,
    generation: i64,
    epoch: i32,
    sequence: i64,
    digest: String,
    position: String,
    replay: bool,
}

fn parse(value: &Value) -> Result<Receipt, &'static str> {
    let r: Receipt = serde_json::from_value(value.clone()).map_err(|_| "typed_receipt")?;
    if r.protocol != "oo.c1.typed-receipt/v3"
        || r.source.is_empty()
        || r.task.is_empty()
        || r.generation < 1
        || r.epoch < 1
        || r.sequence < 1
        || r.digest.len() != 64
        || !r
            .digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || r.position
            .parse::<u64>()
            .ok()
            .is_none_or(|n| n.to_string() != r.position)
    {
        return Err("typed_receipt");
    }
    Ok(r)
}

/// The caller may write the returned frame to the producer only on `Ok`.
/// A timeout/unknown CAS outcome produces no frame; replay the same receipt.
pub async fn acknowledge(client: &Client, target_receipt: &Value) -> Result<Value, &'static str> {
    let r = parse(target_receipt)?;
    let row = client
        .query_one(
            "SELECT _oo.c1_typed_ack($1,$2::text::uuid,$3,$4,$5,$6)",
            &[
                &r.source,
                &r.task,
                &r.generation,
                &r.epoch,
                &r.sequence,
                &r.digest,
            ],
        )
        .await
        .map_err(|_| "typed_checkpoint_unknown")?;
    if !row
        .try_get::<_, bool>(0)
        .map_err(|_| "typed_checkpoint_unknown")?
    {
        return Err("typed_checkpoint_conflict");
    }
    Ok(
        json!({"protocol":"oo.c1.typed-ack/v3","source":r.source,"task":r.task,
        "generation":r.generation,"epoch":r.epoch,"sequence":r.sequence,
        "digest":r.digest,"position":r.position,"replay":r.replay}),
    )
}

/// Oracle capture callers receive exact stored source evidence only AFTER the
/// target checkpoint CAS. This remains a stage ACK, not job/publication success.
pub async fn acknowledge_oracle(client:&Client,target_receipt:&Value)->Result<Value,&'static str> {
    let r=parse(target_receipt)?;
    let mut ack=acknowledge(client,target_receipt).await?;
    let row=client.query_one("SELECT _oo.c1_typed_checkpoint_evidence($1,$2::text::uuid,$3,$4,$5,$6)::text",
        &[&r.source,&r.task,&r.generation,&r.epoch,&r.sequence,&r.digest]).await.map_err(|_|"typed_checkpoint_unknown")?;
    let raw:Option<String>=row.try_get(0).map_err(|_|"typed_checkpoint_unknown")?;
    let source:Value=serde_json::from_str(&raw.ok_or("oracle_checkpoint_absent")?).map_err(|_|"oracle_checkpoint_absent")?;
    bind_oracle_evidence(&mut ack,&r,&source)?;
    ack["source_position"]=source;
    Ok(ack)
}

fn bind_oracle_evidence(ack:&mut Value,r:&Receipt,source:&Value)->Result<(),&'static str>{
    if source["engine"]!="oracle" || source["apply_ordinal"]!=r.position
        || source["source_offset_advanced"]!=false {return Err("oracle_checkpoint_conflict");}
    if source.get("context_sha256").is_some() {
        let position=&source["buffer_position"];
        if source["checkpoint_kind"]!="buffer_consumption" || source["source_offset_write"]!=false
            || position["protocol"]!="oo.c1.oracle-buffer-position/v1"
            || position["source_offset_write"]!=false
            || position["context_sha256"]!=source["context_sha256"]
            || position["bundle_sha256"]!=source["bundle_sha256"]
            || position["typed_batch_digest"]!=r.digest
            || position["cursor_after"]["transaction_ordinal"]!=r.position
            || position["cursor_after"]["transaction_id"]!=position["transaction_id"]
            || ["context_sha256","bundle_sha256","source_evidence_sha256"].into_iter().any(|key|
                source[key].as_str().is_none_or(|s|s.len()!=64 || !s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))))
        {return Err("oracle_checkpoint_conflict");}
        ack["checkpoint_kind"]=json!("buffer_consumption");
        ack["context_sha256"]=source["context_sha256"].clone();
        ack["bundle_sha256"]=source["bundle_sha256"].clone();
        ack["buffer_cursor"]=position["cursor_after"].clone();
        ack["source_evidence_sha256"]=source["source_evidence_sha256"].clone();
        ack["source_offset_write"]=json!(false);
        ack["buffer_checkpoint"]=json!({"protocol":"oo.c1.oracle-buffer-ack/v1","checkpoint_kind":"buffer_consumption",
            "context_sha256":source["context_sha256"],"bundle_sha256":source["bundle_sha256"],
            "cursor":position["cursor_after"],"payload_sha256":source["source_evidence_sha256"],"source_offset_write":false});
    }
    Ok(())
}

/// Reconstruct only an already durable, DB-verified checkpoint for source resume.
/// This does not perform a CAS; callers must prove checkpoint membership first.
pub(crate) fn oracle_checkpoint_ack(receipt: &Value, evidence: &Value) -> Result<Value, &'static str> {
    let r = parse(receipt)?;
    let mut ack = receipt.clone();
    ack["protocol"] = json!("oo.c1.typed-ack/v3");
    bind_oracle_evidence(&mut ack, &r, evidence)?;
    ack["source_position"] = evidence.clone();
    Ok(ack)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_target_receipt_confusion_before_contacting_control() {
        let r = json!({"protocol":"oo.c1.typed-receipt/v3","source":"x",
            "task":"task","generation":1,"epoch":1,"sequence":1,"digest":"a".repeat(64),
            "position":"18446744073709551615","replay":false});
        assert!(parse(&r).is_ok());
        for (key, val) in [
            ("protocol", json!("oo.c1.typed-ack/v3")),
            ("sequence", json!(0)),
            ("position", json!("01")),
            ("digest", json!("A".repeat(64))),
            ("generation", json!(0)),
        ] {
            let mut bad = r.clone();
            bad[key] = val;
            assert!(parse(&bad).is_err());
        }
        let mut bad = r;
        bad["unexpected"] = json!(true);
        assert!(parse(&bad).is_err());
    }

    #[test]
    fn buffer_ack_exposes_consumption_only_and_rejects_mixed_context() {
        let r=parse(&json!({"protocol":"oo.c1.typed-receipt/v3","source":"oracle_unit","task":"task",
            "generation":1,"epoch":1,"sequence":41,"digest":"a".repeat(64),"position":"1","replay":false})).unwrap();
        let evidence=json!({"engine":"oracle","apply_ordinal":"1","source_offset_advanced":false,
            "checkpoint_kind":"buffer_consumption","source_offset_write":false,
            "context_sha256":"b".repeat(64),"bundle_sha256":"c".repeat(64),"source_evidence_sha256":"d".repeat(64),
            "buffer_position":{"protocol":"oo.c1.oracle-buffer-position/v1","source_offset_write":false,
                "context_sha256":"b".repeat(64),"bundle_sha256":"c".repeat(64),"typed_batch_digest":"a".repeat(64),
                "transaction_id":"real_tx","cursor_after":{"transaction_ordinal":"1","transaction_id":"real_tx"}}});
        let mut ack=json!({});
        bind_oracle_evidence(&mut ack,&r,&evidence).unwrap();
        assert_eq!(ack["buffer_cursor"]["transaction_ordinal"],"1");
        assert_eq!(ack["source_offset_write"],false);
        for key in ["source_offset_advanced","source_offset_write"] {
            let mut bad=evidence.clone();bad[key]=json!(true);
            assert!(bind_oracle_evidence(&mut json!({}),&r,&bad).is_err());
        }
        let mut bad=evidence;
        bad["buffer_position"]["context_sha256"]=json!("e".repeat(64));
        assert!(bind_oracle_evidence(&mut json!({}),&r,&bad).is_err());
    }
}
