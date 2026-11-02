//! Closed final captured-buffer scope, not a source resume position.
use serde_json::Value;

pub const SCOPE:&str="oo.source-admission.c1-oracle-buffer-scope/v1";
fn hex(v:&Value)->bool {v.as_str().is_some_and(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)))}
fn decimal(v:&Value)->bool {v.as_str().is_some_and(|s|!s.is_empty()&&s.len()<=38&&!s.starts_with('0')&&s.bytes().all(|b|b.is_ascii_digit()))}

pub fn valid(scope:&Value)->bool {
    let b=&scope["buffer"];let id=&b["source_identity"];let cursor=&b["final_cursor"];
    let Some(pos)=cursor["transaction_ordinal"].as_str().and_then(|s|s.parse::<u64>().ok().filter(|n|n.to_string()==s)) else{return false};
    let Some(context)=b["context_sha256"].as_str() else{return false};
    let prefix=format!("oracle-buffer:{context}:");
    let Some(start)=scope["l0"].as_str().and_then(|s|s.strip_prefix(&prefix))
        .and_then(|s|s.parse::<u64>().ok().filter(|n|n.to_string()==s&&*n<=pos)) else{return false};
    let Some(guid)=id["pdb_guid"].as_str().filter(|s|s.len()==32&&s.bytes().all(|b|b.is_ascii_digit()||(b'A'..=b'F').contains(&b))) else{return false};
    if !decimal(&id["dbid"]) || !decimal(&id["resetlogs_change"]) {return false;}
    let lineage=format!("oracle.{}.{}.{}",id["dbid"].as_str().unwrap(),id["resetlogs_change"].as_str().unwrap(),guid.to_lowercase());
    scope["protocol"]==SCOPE && scope["engine"]=="oracle" && id["engine"]=="oracle"
        && ["pdb_name","db_unique_name"].into_iter().all(|k|id[k].as_str().is_some_and(|s|!s.is_empty()))
        && scope["lineage"]==lineage && scope["source_identity"]["engine"]=="oracle"
        && scope["source_identity"]["measured_id"]==lineage
        && b["checkpoint_kind"]=="buffer_consumption" && b["source_offset_write"]==false
        && ["context_sha256","bundle_sha256","spool_sha256","typed_schema_sha256","connector_offsets_sha256","connector_history_sha256"]
            .into_iter().all(|k|hex(&b[k]))
        && scope["schema_digest"]==b["typed_schema_sha256"] && scope["complete"]==true
        && scope["consistency_grade"]=="T" && scope["cross_table_consistent"]==true
        && (start==0 || start<pos) && scope["l_end"]==format!("oracle-buffer:{context}:{pos}")
        && if pos==0 {cursor["transaction_id"].is_null() && b["commit_scn"].is_null()}
           else {cursor["transaction_id"].as_str().is_some_and(|s|!s.is_empty()) && decimal(&b["commit_scn"])}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn buffer_is_not_a_gtid_or_scn_cursor() {
        let id=json!({"engine":"oracle","dbid":"42","resetlogs_change":"100","pdb_guid":"A".repeat(32),"pdb_name":"P","db_unique_name":"D"});
        let lineage=format!("oracle.42.100.{}","a".repeat(32));
        let b=json!({"checkpoint_kind":"buffer_consumption","context_sha256":"a".repeat(64),"bundle_sha256":"b".repeat(64),
            "spool_sha256":"c".repeat(64),"typed_schema_sha256":"d".repeat(64),"connector_offsets_sha256":"e".repeat(64),
            "connector_history_sha256":"f".repeat(64),"source_identity":id,"source_offset_write":false,
            "final_cursor":{"transaction_ordinal":"4","transaction_id":"txn"},"commit_scn":"2133689"});
        let s=json!({"protocol":SCOPE,"engine":"oracle","lineage":lineage,"source_identity":{"engine":"oracle","measured_id":lineage},
            "buffer":b,"schema_digest":"d".repeat(64),"complete":true,"consistency_grade":"T","cross_table_consistent":true,
            "l0":format!("oracle-buffer:{}:0","a".repeat(64)),"l_end":format!("oracle-buffer:{}:4","a".repeat(64))});
        assert!(valid(&s));
        for position in ["1000001", "18446744073709551615"] {
            let mut large=s.clone();
            large["buffer"]["final_cursor"]["transaction_ordinal"]=json!(position);
            large["l_end"]=json!(format!("oracle-buffer:{}:{position}","a".repeat(64)));
            assert!(valid(&large));
            large["buffer"]["source_offset_write"]=json!(true);
            assert!(!valid(&large));
        }
        for position in ["01000001", "-1", "18446744073709551616"] {
            let mut bad=s.clone();
            bad["buffer"]["final_cursor"]["transaction_ordinal"]=json!(position);
            bad["l_end"]=json!(format!("oracle-buffer:{}:{position}","a".repeat(64)));
            assert!(!valid(&bad));
        }
        for (start,expected) in [("3",true),("04",false),("4",false),("5",false),("-1",false)] {
            let mut changed=s.clone();
            changed["l0"]=json!(format!("oracle-buffer:{}:{start}","a".repeat(64)));
            assert_eq!(valid(&changed),expected,"{start}");
        }
        for (k,v) in [("engine",json!("mysql")),("l_end",json!("2133689")),("consistency_grade",json!("S1"))] {
            let mut bad=s.clone();bad[k]=v;assert!(!valid(&bad));
        }
        let mut bad=s;bad["buffer"]["source_offset_write"]=json!(true);assert!(!valid(&bad));
    }
}
