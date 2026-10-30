//! Verified prior generation and same-task delta recovery; files are not authority.
use super::*;

pub(super) const PARENT_SQL: &str = r#"
SELECT t.receipt::text
FROM _oo.native_c1_baseline_typed t
JOIN _oo.native_c1_baseline_run r USING(source,task_id)
JOIN _oo.source_watermark w ON w.source=r.source
JOIN _oo.source_instance i ON i.source=r.source AND i.source_epoch=r.source_epoch
JOIN jobs.tasks j ON j.id=r.task_id
WHERE r.source=$1 AND r.task_id=$2::text::uuid AND r.engine='oracle'
AND t.acknowledged_at IS NOT NULL AND j.state='succeeded'
AND t.request_digest=t.receipt->>'request_digest'
AND w.source_generation=$3 AND (t.receipt->>'source_generation')::bigint=$3
AND w.target_generation=(t.receipt->>'target_generation')::bigint
AND w.source_epoch=r.source_epoch AND w.applied_pos=r.l_end
AND w.position_codec='oracle-capture-sequence/v1' AND w.position_lineage=r.measured_id
AND i.engine='oracle' AND i.measured_id=r.measured_id
AND i.config_digest=r.config_digest AND i.schema_digest=r.schema_digest
AND t.receipt->'source_instance'->>'measured_id'=r.measured_id
AND t.receipt->'source_instance'->>'config_digest'=r.config_digest
AND t.receipt->'source_instance'->>'schema_digest'=r.schema_digest
AND (t.receipt->>'source_epoch')::integer=r.source_epoch
AND t.receipt->>'applied_pos'=_oo.native_pos_text(r.l_end)
"#;

const CONTROL_PARENT_SQL: &str = r#"
SELECT w.result::text FROM jw_native.source_progress p
JOIN jw_native.watermarks w ON w.task_id=p.task_id AND w.binding_id=p.binding_id
JOIN jw_native.bindings b ON b.id=w.binding_id
JOIN jw_native.pending_acks a ON a.task_id=w.task_id
JOIN jobs.tasks j ON j.id=w.task_id
JOIN jobs.tasks current_job ON current_job.id=p.active_task_id
WHERE p.source_id=$1 AND p.target_database=$2 AND p.source_generation=$3
AND p.task_id=$4::text::uuid AND p.active_task_id=$5::text::uuid
AND b.source_id=p.source_id AND b.target_database=p.target_database AND b.method='C1'
AND w.source_generation=p.source_generation AND a.acknowledged_at IS NOT NULL
AND a.ack_request->>'request_digest'=w.result->>'request_digest'
AND j.state='succeeded' AND j.generation=a.generation AND w.generation=j.generation
AND current_job.state='running' AND current_job.generation=$6
AND current_job.project_id=$7::text::uuid AND current_job.lease_owner=$8
AND current_job.lease_until>clock_timestamp()
"#;

// One MVCC snapshot: never infer a consumed sequence from a position or a file.
pub(super) const RESUME_SQL: &str = r#"
SELECT jsonb_build_object(
 'run',to_jsonb(r)||jsonb_build_object('l0_pos',_oo.native_pos_text(r.l0_pos),
   'applied_pos',_oo.native_pos_text(r.applied_pos),'l_end',_oo.native_pos_text(r.l_end)),
 'instance',to_jsonb(i),'metadata',b.metadata,'sequence',c.seq,
 'receipt',to_jsonb(a)||jsonb_build_object('position',_oo.native_pos_text(a.position)),
 'evidence',e.evidence,'max_sequence',coalesce(n.seq,0),'receipt_count',n.count,
 'all_cdc',coalesce(n.all_cdc,true),'all_fill',coalesce(n.all_fill,true),
 'receipts',coalesce((SELECT jsonb_agg(to_jsonb(fr)||jsonb_build_object('position',_oo.native_pos_text(fr.position)) ORDER BY fr.seq)
   FROM _oo.c1_typed_receipt fr WHERE fr.source=r.source AND fr.task=r.task_id),'[]'::jsonb),
 'buffer_evidences',coalesce((SELECT jsonb_object_agg(bp.seq::text,bp.evidence)
   FROM _oo.c1_typed_buffer_position bp WHERE bp.source=r.source AND bp.task=r.task_id),'{}'::jsonb),
 'format_sealed',EXISTS(SELECT 1 FROM _oo.native_c1_baseline_v2_format f WHERE f.source=r.source AND f.task_id=r.task_id AND f.sealed),
 'initial_target',NOT EXISTS(SELECT 1 FROM _oo.source_watermark w WHERE w.source=r.source AND w.source_generation>0),
 'last_receipt',to_jsonb(last_r)||jsonb_build_object('position',_oo.native_pos_text(last_r.position)),
 'last_evidence',last_e.evidence,
 'tables',(SELECT jsonb_object_agg(name,jsonb_build_object('spec',spec,'complete',snapshot_complete))
   FROM _oo.c1_typed_table WHERE source=r.source AND task=r.task_id),
 'acknowledged',EXISTS(SELECT 1 FROM _oo.native_c1_baseline_typed
   WHERE source=r.source AND task_id=r.task_id AND acknowledged_at IS NOT NULL))::text
FROM _oo.native_c1_baseline_run r
LEFT JOIN _oo.source_instance i ON i.source=r.source AND i.source_epoch=r.source_epoch
LEFT JOIN _oo.c1_typed_capture_bundle b ON b.source=r.source AND b.task=r.task_id
LEFT JOIN _oo.c1_typed_checkpoint c ON c.source=r.source AND c.task=r.task_id
LEFT JOIN _oo.c1_typed_receipt a ON a.source=r.source AND a.task=r.task_id AND a.seq=c.seq
LEFT JOIN _oo.c1_typed_buffer_position e ON e.source=a.source AND e.task=a.task AND e.seq=a.seq
LEFT JOIN LATERAL (SELECT max(seq) seq,count(*) count,bool_and(kind='cdc') all_cdc,bool_and(kind='fill') all_fill
 FROM _oo.c1_typed_receipt WHERE source=r.source AND task=r.task_id) n ON true
LEFT JOIN _oo.c1_typed_receipt last_r ON last_r.source=r.source AND last_r.task=r.task_id AND last_r.seq=n.seq
LEFT JOIN _oo.c1_typed_buffer_position last_e ON last_e.source=last_r.source AND last_e.task=last_r.task AND last_e.seq=last_r.seq
WHERE r.source=$1 AND r.task_id=$2::text::uuid
"#;

fn ordinal(v: &Value) -> Result<u64> {
    let s = v.as_str().ok_or_else(|| Error::new("oracle_cursor"))?;
    s.parse::<u64>()
        .ok()
        .filter(|n| n.to_string() == s)
        .ok_or_else(|| Error::new("oracle_cursor"))
}

impl Buffer {
    pub fn initial_pos(&self) -> Result<u64> {
        ordinal(&self.context["initial_cursor"]["transaction_ordinal"])
    }

    pub fn check_generation(&self, typed: bool, base: i64) -> Result<()> {
        let matches = match &self.rollover {
            None => base == 0,
            Some(d) => base > 0 && d["parent_source_generation"] == base
                && d["source_generation"].as_i64() == base.checked_add(1),
        };
        if !typed || !matches {
            return Err(Error::new("oracle_buffer_generation"));
        }
        Ok(())
    }

    pub(super) fn check_lineage(&self, config: &Value) -> Result<()> {
        let Some(d) = &self.rollover else {
            if self.context.get("rollover").is_some() || self.initial_pos()? != 0 {
                return Err(Error::new("oracle_parent_required"));
            }
            return Ok(());
        };
        let p = &d["previous"];
        let generation = p["source_generation"].as_i64().filter(|n| *n > 0)
            .ok_or_else(|| Error::new("oracle_rollover"))?;
        let parent = document(&d["parent_index"])?;
        let pc = document(&parent["context"])?;
        if d["protocol"] != "oo.oracle.buffer-rollover/v1"
            || d["engine"] != "oracle"
            || d["source_id"] != self.context["source_id"]
            || d["source_generation"].as_i64() != generation.checked_add(1)
            || d["parent_source_generation"] != generation
            || d["parent_index"] != p["buffer_index"]
            || d["parent_ack"] != p["parent_ack"]
            || parent["context"] != p["buffer_context"]
            || parent["final_cursor"] != p["buffer_cursor"]
            || d["initial_cursor"] != p["buffer_cursor"]
            || d["initial_cursor"] != self.context["initial_cursor"]
            || d["final_cursor"] != self.index["final_cursor"]
            || self.context["rollover"] != self.rollover_ref
            || self.bundle["rollover"] != self.rollover_ref
            || self.bundle["boundary"]["stream_mode"] != "delta"
            || self.bundle["boundary"]["initial_position"]
                != p["buffer_cursor"]["transaction_ordinal"]
            || self.bundle["boundary"]["final_position"]
                != self.index["final_cursor"]["transaction_ordinal"]
            || [
                "source_id",
                "source_identity",
                "typed_schema_sha256",
                "snapshot_scn",
            ]
            .iter()
            .any(|k| pc[k] != self.context[k])
            || pc["source_resume"]["connector_history"]["sha256"]
                != self.context["source_resume"]["connector_history"]["sha256"]
            || d["source_resume"]["source_offset_write"] != false
            || d["source_resume"]["reconnect_performed_by_this_tool"] != false
            || self.positions.is_empty()
            || self.index["transaction_count"].as_u64() != Some(self.positions.len() as u64)
            || p["config_digest"]
                != sha256_hex(
                    &serde_json::to_vec(&config["admission_config"])
                        .map_err(|_| Error::new("oracle_config"))?,
                )
        {
            return Err(Error::new("oracle_rollover"));
        }
        let mut pos = self.initial_pos()?;
        let mut cursor = self.context["initial_cursor"].clone();
        for (n, entry) in self.index["transactions"]
            .as_array()
            .ok_or_else(|| Error::new("oracle_index"))?
            .iter()
            .enumerate()
        {
            let frame = document(entry)?;
            let batch = &frame["batch"];
            if batch["kind"] != "cdc"
                || batch["sequence"].as_u64() != Some(n as u64 + 1)
                || ordinal(&batch["previous_position"])? != pos
                || ordinal(&batch["position"])?
                    != pos
                        .checked_add(1)
                        .ok_or_else(|| Error::new("oracle_cursor"))?
                || frame["source_position"]["typed_batch_digest"] != digest(batch)?
                || frame["source_position"]["context_sha256"] != self.context_ref["sha256"]
                || frame["source_position"]["bundle_sha256"] != self.context["bundle"]["sha256"]
                || frame["source_position"]["protocol"] != "oo.c1.oracle-buffer-position/v1"
                || frame["source_position"]["source_offset_write"] != false
                || frame["source_position"]["cursor_before"] != cursor
                || frame["source_position"]["cursor_after"]["transaction_ordinal"]
                    != batch["position"]
                || frame["source_position"]["transaction_id"]
                    .as_str()
                    .is_none_or(str::is_empty)
                || frame["source_position"]["cursor_after"]["transaction_id"]
                    != frame["source_position"]["transaction_id"]
            {
                return Err(Error::new("oracle_delta_sequence"));
            }
            self.check_batch(&frame)?;
            if n == 0 && digest(batch)? != digest(&self.first_batch)? {
                return Err(Error::new("oracle_delta_sequence"));
            }
            pos += 1;
            cursor = frame["source_position"]["cursor_after"].clone();
        }
        if pos != ordinal(&self.index["final_cursor"]["transaction_ordinal"])?
            || cursor != self.index["final_cursor"]
        {
            return Err(Error::new("oracle_delta_sequence"));
        }
        Ok(())
    }

    // Only DB receipts that passed both target and control authority queries enter here.
    fn previous_from_receipts(&self, target: &Value, control: &Value) -> Result<Value> {
        let d = self
            .rollover
            .as_ref()
            .ok_or_else(|| Error::new("oracle_rollover"))?;
        let ack = document(&d["parent_ack"])?;
        let p = &d["previous"];
        let instance = &target["source_instance"];
        let position = &target["oracle_buffer"];
        let pc = document(&p["buffer_context"])?;
        let derived = json!({
            "protocol":"oo.jobs.oracle-buffer-previous/v1","engine":instance["engine"],
            "source_id":target["source_id"],"task_id":instance["lease_id"],
            "source_generation":target["source_generation"],"target_generation":target["target_generation"],
            "source_epoch":target["source_epoch"],"measured_id":instance["measured_id"],
            "config_digest":instance["config_digest"],"schema_digest":instance["schema_digest"],
            "l_end":target["l_end"],"checkpoint_kind":position["checkpoint_kind"],
            "buffer_context":p["buffer_context"],"buffer_index":d["parent_index"],
            "buffer_cursor":position["final_cursor"],"bundle_sha256":position["bundle_sha256"],
            "parent_ack":d["parent_ack"],"request_digest":target["request_digest"]
        });
        if target != control
            || ack["receipt"] != *target
            || derived != *p
            || ack["protocol"] != "oo.oracle.c1-ack-state/v1"
            || ack["source_engine"] != "oracle"
            || ack["acknowledged"] != true
            || ack["task_state"] != "succeeded"
            || [
                "source_id",
                "source_generation",
                "source_epoch",
                "target_generation",
                "request_digest",
                "applied_pos",
            ]
            .iter()
            .any(|k| ack[k] != target[k])
            || target["protocol"] != "oo.pipeline.native-c1-baseline-result/v1"
            || target["request_digest"].as_str().is_none_or(|s| {
                s.len() != 64
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            || target["source_generation"] != d["parent_source_generation"]
            || target["source_id"] != self.context["source_id"]
            || target["source_epoch"].as_i64().is_none_or(|n| n < 1)
            || target["target_generation"].as_i64().is_none_or(|n| n < 1)
            || instance["engine"] != "oracle"
            || instance["measured_id"] != self.measured()?
            || instance["source"] != self.context["source_id"]
            || instance["source_epoch"] != target["source_epoch"]
            || instance["schema_digest"] != self.context["typed_schema_sha256"]
            || instance["lease_id"]
                .as_str()
                .is_none_or(|s| !crate::api::is_uuid(s))
            || position["checkpoint_kind"] != "buffer_consumption"
            || position["source_offset_write"] != false
            || position["context_sha256"] != p["buffer_context"]["sha256"]
            || position["bundle_sha256"] != pc["bundle"]["sha256"]
            || position["source_identity"] != self.context["source_identity"]
            || position["typed_schema_sha256"] != self.context["typed_schema_sha256"]
            || position["connector_history_sha256"]
                != pc["source_resume"]["connector_history"]["sha256"]
            || position["connector_offsets_sha256"]
                != pc["source_resume"]["final_connector_offset"]["sha256"]
            || position["spool_sha256"] != pc["raw_spool"]["sha256"]
            || target["applied_pos"] != p["buffer_cursor"]["transaction_ordinal"]
            || target["l_end"]
                != format!(
                    "oracle-buffer:{}:{}",
                    p["buffer_context"]["sha256"].as_str().unwrap_or(""),
                    self.initial_pos()?
                )
        {
            return Err(Error::new("oracle_unconfirmed_parent"));
        }
        Ok(derived)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn confirmed_previous(
        &self,
        admin: &tokio_postgres::Client,
        control: &tokio_postgres::Client,
        source: &str,
        database: &str,
        task: &str,
        generation: i64,
        project: &str,
        worker: &str,
    ) -> Result<Value> {
        let Some(d) = &self.rollover else {
            return Ok(Value::Null);
        };
        let prior = d["previous"]["task_id"]
            .as_str()
            .filter(|s| crate::api::is_uuid(s))
            .ok_or_else(|| Error::new("oracle_parent_task"))?;
        let parent_generation = d["parent_source_generation"].as_i64().filter(|n| *n > 0)
            .ok_or_else(|| Error::new("oracle_parent"))?;
        let row = admin
            .query_opt(PARENT_SQL, &[&source, &prior, &parent_generation])
            .await
            .map_err(Error::sql)?
            .ok_or_else(|| Error::new("oracle_unconfirmed_parent"))?;
        let target: Value = serde_json::from_str(&row.get::<_, String>(0))
            .map_err(|_| Error::new("oracle_parent"))?;
        let row = control
            .query_opt(
                CONTROL_PARENT_SQL,
                &[
                    &source,
                    &database,
                    &parent_generation,
                    &prior,
                    &task,
                    &generation,
                    &project,
                    &worker,
                ],
            )
            .await
            .map_err(Error::sql)?
            .ok_or_else(|| Error::new("oracle_unconfirmed_parent"))?;
        let confirmed: Value = serde_json::from_str(&row.get::<_, String>(0))
            .map_err(|_| Error::new("oracle_parent"))?;
        self.previous_from_receipts(&target, &confirmed)
    }

    fn payload(&self, sequence: i64) -> Result<&Value> {
        self.positions
            .values()
            .find(|v| v["batch"]["sequence"] == sequence)
            .ok_or_else(|| Error::new("oracle_resume_sequence"))
    }

    pub fn resumed_position(&self, sequence: i64) -> Result<u64> {
        if self.is_initial_fill() {
            if sequence<0||sequence as usize>self.batches.len() {return Err(Error::new("oracle_resume_sequence"));}
            if self.is_initial_mixed() && sequence>0 {
                return ordinal(&self.batches.get(&sequence).ok_or_else(||Error::new("oracle_resume_sequence"))?["position"]);
            }
            return Ok(0);
        }
        if sequence == 0 {
            self.initial_pos()
        } else {
            ordinal(&self.payload(sequence)?["batch"]["position"])
        }
    }

    fn checked_receipt(
        &self,
        stored: &Value,
        evidence: &Value,
        task: &str,
        generation: i64,
        epoch: i64,
    ) -> Result<Value> {
        let seq = stored["seq"]
            .as_i64()
            .ok_or_else(|| Error::new("oracle_resume_receipt"))?;
        let payload = self.payload(seq)?;
        let mut sanitized = payload["source_position"].clone();
        for key in ["captured_event_ids", "end_evidence"] {
            sanitized[key]
                .as_object_mut()
                .ok_or_else(|| Error::new("oracle_resume_receipt"))?
                .remove("path");
        }
        if stored["source"] != self.context["source_id"]
            || stored["task"] != task
            || stored["kind"] != "cdc"
            || stored["digest"] != digest(&payload["batch"])?
            || stored["position"] != payload["batch"]["position"]
            || evidence["buffer_position"] != sanitized
            || evidence["context_sha256"] != self.context_ref["sha256"]
            || evidence["bundle_sha256"] != self.context["bundle"]["sha256"]
            || evidence["source_evidence_sha256"]
                != sha256_hex(
                    &serde_json::to_vec(&payload["source_position"])
                        .map_err(|_| Error::new("oracle_resume_receipt"))?,
                )
        {
            return Err(Error::new("oracle_resume_receipt"));
        }
        let receipt = json!({"protocol":"oo.c1.typed-receipt/v3","source":stored["source"],"task":task,
            "generation":generation,"epoch":epoch,"sequence":seq,"digest":stored["digest"],
            "position":stored["position"],"replay":true});
        crate::c1_typed_ack::oracle_checkpoint_ack(&receipt, evidence).map_err(Error::new)
    }

    fn checked_tables(&self, tables: &Value) -> Result<()> {
        let expected = self.bundle["typed_tables"]
            .as_object()
            .ok_or_else(|| Error::new("oracle_resume_tables"))?;
        let actual = tables
            .as_object()
            .ok_or_else(|| Error::new("oracle_resume_tables"))?;
        if actual.len() != expected.len()
            || expected.iter().any(|(name, spec)| {
                actual
                    .get(name)
                    .is_none_or(|t| t["complete"] != true || t["spec"] != *spec)
            })
        {
            return Err(Error::new("oracle_resume_tables"));
        }
        Ok(())
    }

    fn resume_from_state(
        &self,
        state: &Value,
        previous: &Value,
        task: &str,
        generation: i64,
    ) -> Result<(Value, Option<Value>)> {
        if state.is_null() {
            return Ok((Value::Null, None));
        }
        if self.is_initial_fill() {
            return self.fill_resume_from_state(state,previous,task,generation);
        }
        if self.rollover.is_none() {
            return Err(Error::new("oracle_resume_delta_only"));
        }
        let r = &state["run"];
        let i = &state["instance"];
        let m = &state["metadata"];
        let seq = state["sequence"]
            .as_i64()
            .filter(|s| *s >= 0 && *s as usize <= self.positions.len())
            .ok_or_else(|| Error::new("oracle_resume_sequence"))?;
        let old = r["lease_generation"]
            .as_i64()
            .filter(|g| *g > 0 && *g <= generation)
            .ok_or_else(|| Error::new("oracle_resume_lease"))?;
        let epoch = previous["source_epoch"]
            .as_i64()
            .ok_or_else(|| Error::new("oracle_resume_epoch"))?;
        let max = state["max_sequence"]
            .as_i64()
            .ok_or_else(|| Error::new("oracle_resume_sequence"))?;
        if !matches!(r["state"].as_str(), Some("open" | "awaiting_publish"))
            || r["source"] != self.context["source_id"]
            || r["task_id"] != task
            || r["engine"] != "oracle"
            || r["source_epoch"] != epoch
            || r["config_digest"] != previous["config_digest"]
            || r["schema_digest"] != previous["schema_digest"]
            || r["measured_id"] != self.measured()?
            || r["l0_pos"] != self.context["initial_cursor"]["transaction_ordinal"]
            || r["l0"]
                != format!(
                    "oracle-buffer:{}:{}",
                    self.context_ref["sha256"].as_str().unwrap_or(""),
                    self.initial_pos()?
                )
            || i["source"] != r["source"]
            || i["engine"] != "oracle"
            || i["lease_id"] != task
            || i["lease_generation"] != old
            || i["source_epoch"] != epoch
            || ["measured_id", "config_digest", "schema_digest"]
                .iter()
                .any(|k| i[k] != r[k])
            || m["context_sha256"] != self.context_ref["sha256"]
            || m["bundle_sha256"] != self.context["bundle"]["sha256"]
            || m["buffer_index"] != self.index_ref
            || m["source_id"] != self.context["source_id"]
            || m["source_identity"] != self.context["source_identity"]
            || m["checkpoint_kind"] != "buffer_consumption"
            || m["source_offset_write"] != false
            || state["acknowledged"] != false
            || state["all_cdc"] != true
            || max < seq
            || max > seq + 1
            || state["receipt_count"] != max
        {
            return Err(Error::new("oracle_resume_binding"));
        }
        self.checked_tables(&state["tables"])?;
        if max > 0 {
            self.checked_receipt(
                &state["last_receipt"],
                &state["last_evidence"],
                task,
                old,
                epoch,
            )?;
        }
        let ack = if seq == 0 {
            None
        } else {
            Some(
                json!({"protocol":"oo.jobs.oracle-c1-baseline-ack/v1","stage":"typed_batch","durable":true,
                "receipt":self.checked_receipt(&state["receipt"], &state["evidence"], task, old, epoch)?}),
            )
        };
        if r["state"] == "awaiting_publish"
            && (seq as usize != self.positions.len()
                || max != seq
                || ordinal(&r["l_end"])? != self.resumed_position(seq)?)
        {
            return Err(Error::new("oracle_resume_unsealed"));
        }
        Ok((
            json!({"protocol":"oo.jobs.oracle-buffer-resume/v1","engine":"oracle",
            "source_id":self.context["source_id"],"task_id":task,
            "source_generation":self.rollover.as_ref().ok_or_else(||Error::new("oracle_rollover"))?["source_generation"],"lease_generation":old,
            "source_epoch":epoch,"config_digest":previous["config_digest"],"buffer_index":self.index_ref,
            "buffer_context":self.context_ref,"bundle_sha256":self.context["bundle"]["sha256"],
            "state":r["state"],"last_ack":null,"resume_sequence":seq}),
            ack,
        ))
    }

    pub async fn resume(
        &self,
        admin: &tokio_postgres::Client,
        previous: &Value,
        task: &str,
        generation: i64,
        output: &str,
    ) -> Result<Value> {
        let source = self.context["source_id"]
            .as_str()
            .ok_or_else(|| Error::new("oracle_source"))?;
        let claimed: bool = admin.query_one("SELECT EXISTS(SELECT 1 FROM jobs.tasks WHERE id=$1::text::uuid AND generation=$2 AND state='running' AND lease_until>clock_timestamp())",
            &[&task,&generation]).await.map_err(Error::sql)?.get(0);
        if !claimed {
            return Err(Error::new("oracle_resume_lease"));
        }
        let row = admin
            .query_opt(RESUME_SQL, &[&source, &task])
            .await
            .map_err(Error::sql)?;
        let state = row
            .map(|r| serde_json::from_str::<Value>(&r.get::<_, String>(0)))
            .transpose()
            .map_err(|_| Error::new("oracle_resume"))?
            .unwrap_or(Value::Null);
        let (mut resume, ack) = self.resume_from_state(&state, previous, task, generation)?;
        if self.is_initial_fill() && !resume.is_null() {
            let epoch=resume["source_epoch"].as_i64().ok_or_else(||Error::new("oracle_fill_epoch"))?;
            let seq=resume["resume_sequence"].as_i64().ok_or_else(||Error::new("oracle_fill_sequence"))?;
            let raw:String=admin.query_one(
                "SELECT _oo.c1_oracle_fill_rebind($1,$2::text::uuid,$3,$4::bigint::integer,$5::text::jsonb,$6)::text",
                &[&source,&task,&generation,&epoch,&resume["initial_fill"].to_string(),&seq])
                .await.map_err(Error::sql)?.get(0);
            let proof:Value=serde_json::from_str(&raw).map_err(|_|Error::new("oracle_fill_rebind"))?;
            if proof["source_epoch"]!=epoch||proof["resumed_sequence"]!=seq
                ||proof["buffer_context_sha256"]!=self.context_ref["sha256"]
                ||proof["bundle_sha256"]!=self.context["bundle"]["sha256"]
                ||proof["completed_tables"]!=resume["completed_tables"] {
                return Err(Error::new("oracle_fill_rebind"));
            }
        }
        if let Some(ack) = ack {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let bytes = serde_json::to_vec(&ack).map_err(|_| Error::new("oracle_resume_ack"))?;
            let path = format!("{output}.resume-ack.json");
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .map_err(|_| Error::new("oracle_resume_ack"))?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| Error::new("oracle_resume_ack"))?;
            resume["last_ack"] =
                json!({"path":path,"sha256":sha256_hex(&bytes),"bytes":bytes.len()});
        }
        Ok(resume)
    }

    fn fill_resume_from_state(&self,state:&Value,previous:&Value,task:&str,generation:i64)->Result<(Value,Option<Value>)> {
        let plan=self.initial_fill.as_ref().ok_or_else(||Error::new("oracle_fill_only"))?;
        let mixed=self.is_initial_mixed();
        let final_pos=if mixed {plan["final_position"].as_str().ok_or_else(||Error::new("oracle_mixed_cursor"))?}else{"0"};
        let (r,i,m)=(&state["run"],&state["instance"],&state["metadata"]);
        let seq=state["sequence"].as_i64().filter(|n|*n>=0&&*n as usize<=self.batches.len()).ok_or_else(||Error::new("oracle_fill_sequence"))?;
        let max=state["max_sequence"].as_i64().filter(|n|*n>=seq&&*n<=seq+1&&*n as usize<=self.batches.len()).ok_or_else(||Error::new("oracle_fill_sequence"))?;
        let old=r["lease_generation"].as_i64().filter(|n|*n>0&&*n<=generation).ok_or_else(||Error::new("oracle_fill_lease"))?;
        let epoch=r["source_epoch"].as_i64().filter(|n|*n>0).ok_or_else(||Error::new("oracle_fill_epoch"))?;
        let canonical_l0=format!("oracle-buffer:{}:0",self.context_ref["sha256"].as_str().unwrap_or(""));
        if !previous.is_null()||!matches!(r["state"].as_str(),Some("open"|"awaiting_publish"))
            ||r["source"]!=self.context["source_id"]||r["task_id"]!=task||r["engine"]!="oracle"
            ||r["l0_pos"]!="0"||r["applied_pos"]!=if r["state"]=="awaiting_publish" {final_pos}else{"0"}
            ||(r["l0"]!=self.context["bundle"]["sha256"]&&r["l0"]!=canonical_l0)
            ||r["config_digest"]!=plan["config_digest"]||r["schema_digest"]!=plan["typed_schema_sha256"]
            ||r["measured_id"]!=self.measured()?
            ||i["source"]!=r["source"]||i["engine"]!="oracle"||i["lease_id"]!=task
            ||i["lease_generation"]!=old||i["source_epoch"]!=epoch
            ||["measured_id","config_digest","schema_digest"].iter().any(|k|i[k]!=r[k])
            ||m["initial_fill"]!=*plan||m["buffer_index"]!=self.index_ref
            ||m["context_sha256"]!=self.context_ref["sha256"]||m["bundle_sha256"]!=self.context["bundle"]["sha256"]
            ||m["source_identity"]!=self.context["source_identity"]||m["source_id"]!=self.context["source_id"]
            ||m["checkpoint_kind"]!="buffer_consumption"||m["source_offset_write"]!=false
            ||(!mixed&&state["all_fill"]!=true)||state["receipt_count"]!=max
            ||state["initial_target"]!=true||state["acknowledged"]!=false {
            return Err(Error::new("oracle_fill_resume_binding"));
        }
        let receipts=state["receipts"].as_array().ok_or_else(||Error::new("oracle_fill_receipts"))?;
        if receipts.len()!=max as usize {return Err(Error::new("oracle_fill_receipts"));}
        for (index,stored) in receipts.iter().enumerate() {
            let sequence=(index+1) as i64;
            let batch=self.batches.get(&sequence).ok_or_else(||Error::new("oracle_fill_receipts"))?;
            if stored["source"]!=self.context["source_id"]||stored["task"]!=task||stored["seq"]!=sequence
                ||stored["kind"]!=batch["kind"]||stored["position"]!=batch["position"]||stored["digest"]!=digest(batch)? {
                return Err(Error::new("oracle_fill_receipts"));
            }
            if mixed&&batch["kind"]=="cdc" {
                self.checked_receipt(stored,&state["buffer_evidences"][sequence.to_string()],task,old,epoch)?;
            }
        }
        let completed=self.fill_tables(&state["tables"],seq)?;
        if mixed && max>plan["fill_batch_count"].as_i64().ok_or_else(||Error::new("oracle_mixed_plan"))?
            && completed.len()!=plan["tables"].as_object().ok_or_else(||Error::new("oracle_fill_tables"))?.len() {
            return Err(Error::new("oracle_mixed_incomplete_snapshot"));
        }
        if (r["state"]=="awaiting_publish"&&(seq as usize!=self.batches.len()||max!=seq
            ||completed.len()!=plan["tables"].as_object().ok_or_else(||Error::new("oracle_fill_tables"))?.len()
            ||r["l_end"]!=final_pos||state["format_sealed"]!=true))
            ||(r["state"]=="open"&&state["format_sealed"]!=false) {
            return Err(Error::new("oracle_fill_unsealed"));
        }
        let ack=if seq==0 {None}else{
            let stored=&receipts[seq as usize-1];
            if state["receipt"]!=*stored {return Err(Error::new("oracle_fill_checkpoint_receipt"));}
            let receipt=if mixed&&stored["kind"]=="cdc" {
                self.checked_receipt(stored,&state["evidence"],task,old,epoch)?
            }else{
                json!({"protocol":"oo.c1.typed-ack/v3","source":r["source"],"task":task,"generation":old,
                    "epoch":epoch,"sequence":seq,"position":"0","digest":stored["digest"],"replay":true})
            };
            Some(json!({"protocol":"oo.jobs.oracle-c1-baseline-ack/v1","stage":"typed_batch","durable":true,"receipt":receipt}))
        };
        Ok((json!({"protocol":"oo.jobs.oracle-fill-resume/v1","engine":"oracle","source_id":r["source"],
            "task_id":task,"source_generation":1,"lease_generation":old,"source_epoch":epoch,
            "config_digest":plan["config_digest"],"schema_digest":plan["typed_schema_sha256"],
            "buffer_index":self.index_ref,"buffer_context":self.context_ref,"bundle_sha256":self.context["bundle"]["sha256"],
            "state":r["state"],"initial_fill":plan,"completed_tables":completed,"resume_sequence":seq,"last_ack":null}),ack))
    }

    pub fn delta_open(
        &self,
        frame: &Value,
        previous: &Value,
        sequence: i64,
        epoch: i64,
        request: &mut Value,
    ) -> Result<()> {
        if self.rollover.is_none() {
            return Ok(());
        }
        if frame["stream_mode"] != "delta"
            || frame["rollover"] != self.rollover_ref
            || frame["buffer_index"] != self.index_ref
            || frame["parent_l_end"] != previous["l_end"]
            || frame["resume_sequence"] != sequence
            || previous["source_epoch"] != epoch
            || previous["config_digest"] != request["config_digest"]
            || previous["schema_digest"] != request["schema_digest"]
            || previous["measured_id"] != request["measured_id"]
        {
            return Err(Error::new("oracle_delta_open"));
        }
        request["stream_mode"] = json!("delta");
        request["rollover"] = self.rollover_ref.clone();
        request["buffer_index"] = self.index_ref.clone();
        request["previous"] = previous.clone();
        request["resume_sequence"] = json!(sequence);
        Ok(())
    }

    pub fn open_ack(
        &self,
        receipt: &Value,
        previous: &Value,
        sequence: i64,
        epoch: i64,
        reply: &mut Value,
    ) -> Result<()> {
        if self.rollover.is_none() {
            return Ok(());
        }
        if receipt["outcome"] != "open"
            || receipt["rollover_sha256"] != self.rollover_ref["sha256"]
            || receipt["seeded_from_request_digest"] != previous["request_digest"]
            || receipt["resumed_sequence"] != sequence
            || receipt["source_epoch"] != epoch
        {
            return Err(Error::new("oracle_stage_open_proof"));
        }
        for k in [
            "rollover_sha256",
            "seeded_from_request_digest",
            "resumed_sequence",
            "source_epoch",
        ] {
            reply[k] = receipt[k].clone();
        }
        Ok(())
    }

    pub async fn seeded_tables(
        &self,
        admin: &tokio_postgres::Client,
        task: &str,
        generation: i64,
        epoch: i64,
    ) -> Result<std::collections::BTreeSet<String>> {
        let source = self.context["source_id"]
            .as_str()
            .ok_or_else(|| Error::new("oracle_source"))?;
        let row = admin.query_one(
            "SELECT jsonb_object_agg(t.name,jsonb_build_object('spec',t.spec,'complete',t.snapshot_complete))::text FROM _oo.c1_typed_table t JOIN _oo.native_c1_baseline_run r ON r.source=t.source AND r.task_id=t.task JOIN jobs.tasks j ON j.id=t.task WHERE t.source=$1 AND t.task=$2::text::uuid AND r.lease_generation=$3 AND r.source_epoch::bigint=$4 AND j.state='running' AND j.generation=$3 AND j.lease_until>clock_timestamp()",
            &[&source,&task,&generation,&epoch]).await.map_err(Error::sql)?;
        let raw: Option<String> = row.get(0);
        let tables: Value =
            serde_json::from_str(&raw.ok_or_else(|| Error::new("oracle_seed_tables"))?)
                .map_err(|_| Error::new("oracle_seed_tables"))?;
        self.checked_tables(&tables)?;
        Ok(tables
            .as_object()
            .ok_or_else(|| Error::new("oracle_seed_tables"))?
            .keys()
            .cloned()
            .collect())
    }
}

#[cfg(test)]
#[path = "native_oracle_delta_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "native_oracle_fill_tests.rs"]
mod fill_tests;
