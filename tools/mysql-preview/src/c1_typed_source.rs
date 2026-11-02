//! Opt-in typed-key slice over the existing ROW/FULL/GTID transaction assembler.
//! No target connection, source writes, automatic fallback, or ACKs live here.
use super::*;
use mysql::chunk::{ChunkBound, ChunkRead, KeyColumn};
use mysql::map::{MappedColumn, MysqlColumn};
use mysql::values::{self, RawCell};
use oo_connectors::{
    mysql,
    types::{RunBinding, assemble_batch},
};
use oo_pipeline::c1_typed::{Batch, Column, PROTOCOL, Part, TableSpec};
#[path = "c1_export_parts.rs"]
mod export_parts;
#[path = "c1_export_resume.rs"]
mod export_resume;
#[path = "c1_initial_source.rs"]
mod initial_source;

pub(crate) fn initial_resume_enabled(config: &Value) -> Result<bool, PreviewError> {
    initial_source::enabled(config)
}

struct Table {
    catalog: Vec<MysqlColumn>,
    columns: Vec<MappedColumn>,
    keys: Vec<KeyColumn>,
    spec: TableSpec,
}

pub(crate) fn export_row_limit(config:&Value)->Result<Option<u64>,PreviewError> {
    let Some(policy)=config.get("c1_export_parts") else {return Ok(None)};
    if policy.as_object().is_none_or(|o|o.len()!=1 || !o.contains_key("max_rows")) {return Err(fault());}
    Ok(Some(policy["max_rows"].as_u64().filter(|n|*n>0&&*n<=50_000_000).ok_or_else(fault)?))
}

pub struct Capture {
    cfg: C1Config,
    live: Live,
    stream: Pin<Box<BinlogStream>>,
    out: PathBuf,
    tables: BTreeMap<String, Table>,
    binding: RunBinding,
    pub l0: String,
    observed_gtid: String,
    preflight: Value,
    cursor_evidence: Value,
}

impl Capture {
    /// The source position is sampled before any chunk; the real binlog reader
    /// subscribes before this returns. Retained logs are replayed by next().
    pub async fn open(
        config: &Path,
        out: &Path,
        source: &str,
        generation: u64,
    ) -> Result<Self, PreviewError> {
        Self::open_at(config, out, source, generation, None, false).await
    }

    /// Reconnect at a caller-confirmed committed GTID, never at a newly sampled L0.
    pub async fn continue_from(
        config: &Path,
        out: &Path,
        source: &str,
        generation: u64,
        checkpoint: &str,
    ) -> Result<Self, PreviewError> {
        Self::open_at(config, out, source, generation, Some(checkpoint), false).await
    }

    async fn open_at(
        config: &Path,
        out: &Path,
        source: &str,
        generation: u64,
        checkpoint: Option<&str>,
        retained: bool,
    ) -> Result<Self, PreviewError> {
        let cfg = load_config(config)?;
        configured_selection(&cfg)?;
        if retained {
            if !out.join("typed-session.json").is_file() {
                return Err(PreviewError::new("c1", "typed_journal_missing"));
            }
        } else {
            fresh_dir(out)?;
        }
        let mut probe = connect_cdc(&cfg).await?;
        let vars = preflight_values(&mut probe).await?;
        if vars["server_id"]
            .as_str()
            .and_then(|s| s.parse::<u32>().ok())
            == Some(cfg.server_id)
        {
            return Err(PreviewError::new("config", "server_id"));
        }
        replication_grants_ok(&rows_first(
            &probe
                .query::<Row, _>("SHOW GRANTS FOR CURRENT_USER()")
                .await
                .map_err(mysql_error)?,
        )?)?;
        let (pk, names, catalog) = load_pk(&mut probe, &cfg.tables).await?;
        let kinds = load_column_kinds(&mut probe, &cfg.tables).await?;
        let visible = visible_ordinals(&cfg, &names)?;
        let reserved = oo_connectors::PG17_RESERVED
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut tables = BTreeMap::new();
        for qualified in &cfg.tables {
            let (schema, table) = qualified.split_once('.').ok_or_else(fault)?;
            let selected = mysql::SelectedTable {
                schema: schema.into(),
                name: table.into(),
                columns: cfg.columns[qualified].clone(),
            };
            let catalog = mysql::catalog_columns(&mut probe, &mut Vec::new(), &selected).await?;
            let mut columns = mysql::keep_requested_keyed(
                mysql::map::map_keyed_table(&catalog, false, true, &reserved)?,
                &selected.columns,
            )?;
            for (i, col) in columns.iter_mut().enumerate() {
                col.plan.attnum = i as i32 + 1;
            }
            // Float/JSON still lack a lossless binlog transport. Temporal values
            // share the snapshot codec and a verified UTC source session.
            if columns.iter().any(|c| {
                !matches!(
                    c.store,
                    mysql::map::StoreKind::Int { .. }
                        | mysql::map::StoreKind::Numeric { .. }
                        | mysql::map::StoreKind::Text { .. }
                        | mysql::map::StoreKind::Bytes
                        | mysql::map::StoreKind::Date
                        | mysql::map::StoreKind::Time
                        | mysql::map::StoreKind::TimestampNaive
                        | mysql::map::StoreKind::TimestampTz
                )
            }) {
                return Err(PreviewError::new("catalog", "typed_source_type"));
            }
            let keys = mysql::chunk::key_columns(&mut probe, schema, table).await?;
            if keys.len() != pk[qualified].len()
                || keys
                    .iter()
                    .any(|k| !columns.iter().any(|c| c.key && c.plan.name == k.name))
            {
                return Err(PreviewError::new("catalog", "primary_key"));
            }
            let spec = TableSpec {
                columns: columns
                    .iter()
                    .map(|c| Column {
                        name: c.plan.name.clone(),
                        oid: c.oid,
                        typmod: c.typmod,
                        nullable: !c.not_null,
                    })
                    .collect(),
                keys: keys
                    .iter()
                    .map(|k| {
                        columns
                            .iter()
                            .position(|c| c.plan.name == k.name)
                            .ok_or_else(fault)
                    })
                    .collect::<Result<_, _>>()?,
            };
            spec.validate().map_err(|_| fault())?;
            tables.insert(
                qualified.clone(),
                Table {
                    catalog,
                    columns,
                    keys,
                    spec,
                },
            );
        }
        let (file, _, executed) = full_status(&mut probe).await?;
        let l0 = checkpoint.unwrap_or(&executed).to_owned();
        let span = parse_span(&l0)?;
        let uuid = scalar(&mut probe, "SELECT @@GLOBAL.server_uuid").await?;
        let purged = scalar(&mut probe, "SELECT @@GLOBAL.gtid_purged").await?;
        validate_checkpoint(&l0, &executed, &purged, &uuid)?;
        let cursor_evidence=json!({"start":l0,"executed":executed,"purged":purged,
            "source_uuid":uuid,"verified_at_unix_ms":now_ms()?});
        let schema_hash = sha256_hex(catalog.as_bytes());
        let mut live = Live {
            assembler: Assembler::new(),
            span,
            file,
            epoch: 1,
            bind: String::new(),
            schema_hash: schema_hash.clone(),
            type_hash: schema_hash.clone(),
            pk,
            selected: cfg.tables.iter().cloned().collect(),
            visible,
            column_names: names,
            column_kinds: kinds,
            selected_columns: cfg.columns.clone(),
            allow_nullable: true,
            pending: None,
            buf: Vec::new(),
            counts: BTreeMap::new(),
            globals: 0,
            rows: Vec::new(),
            txn_budget: TxnBudget::default(),
            foreign: false,
            source_commit_timestamp: None,
            out: out.into(),
        };
        live.bind = bind_line(
            &sha256_hex(uuid.as_bytes()),
            &schema_hash,
            &schema_hash,
            &schema_hash,
            &cfg.tables,
        );
        live.feed(&live.bind.clone())?;
        let sid = Sid::from_str(&l0).map_err(|_| fault())?;
        let stream = connect_cdc(&cfg)
            .await?
            .get_binlog_stream(
                BinlogStreamRequest::new(cfg.server_id)
                    .with_gtid()
                    .with_gtid_set([sid]),
            )
            .await
            .map_err(mysql_error)?;
        Ok(Self {
            cfg,
            live,
            stream: Box::pin(stream),
            out: out.into(),
            tables,
            binding: RunBinding {
                source_id: source.into(),
                schema: String::new(),
                table: String::new(),
                run_generation: generation,
                input_digest: [0; 32],
            },
            l0,
            observed_gtid: executed,
            preflight: vars,
            cursor_evidence,
        })
    }

    pub fn specs(&self) -> BTreeMap<String, TableSpec> {
        self.tables
            .iter()
            .map(|(n, t)| (n.clone(), t.spec.clone()))
            .collect()
    }

    pub fn initial_position(&self) -> u64 {
        parse_span(&self.l0).expect("validated L0").end
    }

    /// Source-side lexicographic predicate and ORDER BY preserve MySQL collation.
    /// The caller applies/ACKs each returned part before requesting the next one.
    pub async fn snapshot_chunk(
        &self,
        name: &str,
        lo: Option<Vec<mysql::chunk::KeyValue>>,
        rows: u64,
        sequence: i64,
    ) -> Result<Option<(Batch, Vec<mysql::chunk::KeyValue>)>, PreviewError> {
        self.snapshot_chunk_to(name,lo,rows,sequence,self.out.join(format!("fill-{sequence:08}.arrow"))).await
    }

    async fn snapshot_chunk_to(
        &self,name:&str,lo:Option<Vec<mysql::chunk::KeyValue>>,rows:u64,sequence:i64,path:PathBuf,
    ) -> Result<Option<(Batch, Vec<mysql::chunk::KeyValue>)>, PreviewError> {
        let t = self.tables.get(name).ok_or_else(fault)?;
        let (schema, table) = name.split_once('.').ok_or_else(fault)?;
        let mut conn = connect_cdc(&self.cfg).await?;
        replication_grants_ok(&rows_first(
            &conn
                .query::<Row, _>("SHOW GRANTS FOR CURRENT_USER()")
                .await
                .map_err(mysql_error)?,
        )?)?;
        let Some(hi) =
            mysql::chunk::next_boundary(&mut conn, schema, table, &t.keys, lo.clone(), rows)
                .await?
        else {
            return Ok(None);
        };
        let mut binding = self.binding.clone();
        binding.schema = schema.into();
        binding.table = table.into();
        let (_, batch) = mysql::chunk::read_chunk(
            conn,
            ChunkRead {
                schema,
                table,
                keys: &t.keys,
                columns: &t.columns,
                binding: &binding,
                bound: &ChunkBound {
                    lo,
                    hi: Some(hi.clone()),
                },
                max_rows: rows,
                max_bytes: 16 * 1024 * 1024,
                deadline: Duration::from_secs(30),
            },
        )
        .await?;
        crate::c1_chunked::write_part(&path, &batch.typed)?;
        let part = part(name, &t.spec, path)?;
        let pos = self.initial_position().to_string();
        Ok(Some((
            Batch {
                protocol: PROTOCOL.into(),
                sequence,
                previous_position: pos.clone(),
                position: pos,
                kind: "fill".into(),
                parts: vec![part],
            },
            hi,
        )))
    }

    /// One complete committed transaction; PK moves, delete/reinsert, rollback,
    /// GTID gaps and DDL keep the original Live/Assembler handling.
    pub async fn next(&mut self, sequence: i64, wait: Duration) -> Result<Batch, PreviewError> {
        self.next_named(sequence,wait,"cdc").await
    }

    async fn next_named(&mut self, sequence:i64,wait:Duration,prefix:&str)->Result<Batch,PreviewError> {
        let previous = self.live.span.end;
        timeout(wait, async {
            while self.live.span.end == previous {
                let event = poll_fn(|cx| self.stream.as_mut().poll_next(cx))
                    .await
                    .ok_or_else(fault)?
                    .map_err(mysql_error)?;
                self.live.on_event(event, self.stream.as_ref().get_ref())?;
            }
            let pos = self.live.span.end;
            let mut by_table: BTreeMap<String, BTreeMap<Vec<u8>, Seen>> = BTreeMap::new();
            // Preserve source event order while collapsing only the final image
            // for the exact typed key. Do not collapse across transactions.
            for row in std::mem::take(&mut self.live.rows) {
                let t = self.tables.get(&row.table).ok_or_else(fault)?;
                let cells = decode_row(&row, &t.columns)?;
                let key = t
                    .spec
                    .keys
                    .iter()
                    .map(|&k| oo_pipeline::c1_key::source_component(&cells[k]))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| fault())?;
                let id = oo_pipeline::c1_key::identity(&key).map_err(|_| fault())?;
                by_table
                    .entry(row.table.clone())
                    .or_default()
                    .insert(id, row);
            }
            let mut parts = Vec::new();
            for (index, (name, rows)) in by_table.into_iter().enumerate() {
                let t = &self.tables[&name];
                let mut columns = t.columns.clone();
                let markers = [
                    MysqlColumn {
                        name: "_oo_pos".into(),
                        ordinal: columns.len() as i32 + 1,
                        data_type: "decimal".into(),
                        column_type: "decimal(38,0)".into(),
                        nullable: false,
                        char_len: None,
                        precision: Some(38),
                        scale: Some(0),
                        charset: None,
                        column_key: String::new(),
                    },
                    MysqlColumn {
                        name: "_oo_op".into(),
                        ordinal: columns.len() as i32 + 2,
                        data_type: "varchar".into(),
                        column_type: "varchar(8)".into(),
                        nullable: false,
                        char_len: Some(8),
                        precision: None,
                        scale: None,
                        charset: Some("utf8mb4".into()),
                        column_key: String::new(),
                    },
                ];
                for m in markers {
                    columns.push(mysql::map::map_column(&m, false, true, &BTreeSet::new())?);
                }
                let mut data = Vec::new();
                for row in rows.into_values() {
                    let mut cells = decode_row(&row, &t.columns)?;
                    for (col, raw) in columns[t.columns.len()..].iter().zip([
                        RawCell::Bytes(pos.to_string().into_bytes()),
                        RawCell::Bytes(if row.deleted {
                            b"delete".to_vec()
                        } else {
                            b"upsert".to_vec()
                        }),
                    ]) {
                        cells.push(values::decode_cell(col.store, &raw, false)?.0);
                    }
                    data.push(cells);
                }
                let (schema, table) = name.split_once('.').ok_or_else(fault)?;
                let mut binding = self.binding.clone();
                binding.schema = schema.into();
                binding.table = table.into();
                let typed = assemble_batch(
                    &binding,
                    columns.iter().map(|c| c.plan.clone()).collect(),
                    &t.spec
                        .keys
                        .iter()
                        .map(|k| *k as i32 + 1)
                        .collect::<Vec<_>>(),
                    data,
                )
                .map_err(|_| fault())?;
                let path = self.out.join(format!("{prefix}-{sequence:08}-{index}.arrow"));
                crate::c1_chunked::write_part(&path, &typed)?;
                parts.push(part(&name, &t.spec, path)?);
            }
            let batch = Batch {
                protocol: PROTOCOL.into(),
                sequence,
                previous_position: previous.to_string(),
                position: pos.to_string(),
                kind: "cdc".into(),
                parts,
            };
            batch.validate().map_err(|_| fault())?;
            Ok(batch)
        })
        .await
        .map_err(|_| PreviewError::new("budget", "typed_cdc_deadline"))?
    }
}

/// Fixed v3 producer argv. The worker owns credentials for the target and the
/// real claim; this process owns only source credentials and Arrow part files.
pub async fn run_stdio(config:&Path,out:&Path)->Result<(),PreviewError> {
    let mut input=BufReader::new(std::io::stdin().lock());
    let mut line=String::new();
    input.read_line(&mut line).map_err(|_|fault())?;
    if line.len()>1024*1024 {return Err(fault());}
    let mut context:Value=serde_json::from_str(&line).map_err(|_|fault())?;
    if context["protocol"]!="oo.jobs.mysql-c1-bootstrap/v1" {return Err(fault());}
    let identity_input:Value=serde_json::from_slice(&fs::read(config).map_err(|_|fault())?).map_err(|_|fault())?;
    let source_registration=identity::bind_context(&identity_input,&context)?;
    if !context["sealed_export"].is_null() {
        return export_resume::run(config,out,&mut input,&mut context).await;
    }
    if context["initial_protocol"]==initial_source::PROTOCOL {
        return initial_source::run(config,out,&mut input,&mut context).await;
    }
    let delta = Delta::parse(&context)?;
    let source=context["source_id"].as_str().ok_or_else(fault)?.to_owned();
    let generation=context["source_generation"].as_u64().filter(|n|*n>0).ok_or_else(fault)?;
    let input_config:Value=serde_json::from_slice(&fs::read(config).map_err(|_|fault())?).map_err(|_|fault())?;
    if initial_source::enabled(&input_config)? && delta.is_none() {
        return Err(PreviewError::new("c1","initial_resume_worker_required"));
    }
    if input_config.get("owned_retained_fixture").is_some() {
        return Err(PreviewError::new("config","retained_fixture_not_a_c1_baseline"));
    }
    let saved_deadline=if delta.as_ref().is_some_and(|d|d.resumed) {
        let saved=read_document(&out.join("typed-session.json"))?["deadline_unix_ms"]
            .as_u64().ok_or_else(||PreviewError::new("c1","typed_journal_deadline"))?;
        Some(saved)
    } else {None};
    let deadline=cycle_clock::deadline(&input_config,&context,saved_deadline,now_ms()?)?;
    let check=||if now_ms()?<deadline {Ok(())}else{Err(PreviewError::new("budget","cycle_deadline"))};
    check()?;
    let pending = match &delta {
        Some(d) if d.resumed => d.pending(out)?,
        _ => None,
    };
    let start = delta.as_ref().map(|d| {
        let position = pending.as_ref().and_then(|f| f.pointer("/batch/position"))
            .and_then(Value::as_str).unwrap_or(&d.position);
        format!("{}:1-{position}", d.l0.split(':').next().unwrap_or(""))
    });
    let mut capture=Capture::open_at(config,out,&source,generation,start.as_deref(),
        delta.as_ref().is_some_and(|d|d.resumed)).await?;
    if let Some(d) = &delta { capture.l0 = d.l0.clone(); }
    let l0=capture.l0.clone();
    let lineage=format!("mysql.{}",capture.live.span.uuid);
    super::handoff::validate(&input_config,&context,&capture.live.span.uuid)?;
    context["source_preflight"]=capture.preflight.clone();
    context["secret_ref"]=json!(capture.cfg.secret_ref);
    let tables=capture.tables.iter().map(|(name,t)|{
        let (schema,table)=name.split_once('.').ok_or_else(fault)?;
        Ok((schema.to_owned(),table.to_owned(),t.columns.clone()))
    }).collect::<Result<Vec<_>,PreviewError>>()?;
    let schemas=json!(tables.iter().map(|(_,table,cols)|Ok(json!({"table":table,
        "columns":crate::c1_materialize::schema_columns(cols)?}))).collect::<Result<Vec<_>,PreviewError>>()?);
    let mut admission=crate::c1_materialize::admission_config(&context,&capture.cfg.tables);
    let export_limit=export_row_limit(&input_config)?;
    if let Some(limit)=export_limit {
        admission["budget"]["hard_limit"]=json!(limit);
    }
    let config_digest=sha256_hex(&serde_json::to_vec(&admission).map_err(|_|fault())?);
    let schema_digest=sha256_hex(&serde_json::to_vec(&schemas).map_err(|_|fault())?);
    let export_metadata=if export_limit.is_some() {
        let metadata_path=out.join("export-metadata.json");
        if !metadata_path.exists() {
            let writes=reader_write_audit(&capture.cfg,out).await?;
            let metadata=json!({"protocol":"oo.mysql.export-metadata/v1",
                "measurement_phase":"before_snapshot","source_connected":true,"measured_at_unix_ms":now_ms()?,
                "source_id":source,"task_id":context["lease_id"],"l0":l0,"source_uuid":capture.live.span.uuid,
                "connection_config_sha256":sha256_hex(&fs::read(config).map_err(|_|fault())?),
                "source_preflight":capture.preflight,"reader_write_attempts":writes,
                "tables":capture.tables.iter().map(|(name,t)|(name,json!({"catalog":t.catalog,"keys":t.keys}))).collect::<BTreeMap<_,_>>()});
            durable_document(&metadata_path,&metadata)?;
        }
        let bytes=fs::read(&metadata_path).map_err(|_|fault())?;
        Some(json!({"path":metadata_path,"sha256":sha256_hex(&bytes)}))
    } else {None};
    if let Some(d) = &delta {
        let previous = &context["previous"];
        if previous["measured_id"] != lineage || previous["config_digest"] != config_digest
            || previous["schema_digest"] != schema_digest {
            return Err(PreviewError::new("c1", "typed_previous_identity"));
        }
        if let Some(request)=quiet_request(&context,&capture.observed_gtid,
            &sha256_hex(&fs::read(config).map_err(|_|fault())?)) {
            send_frame(&json!({"kind":"quiet","request":request}))?;
            return Ok(());
        }
        let header = json!({"protocol":"oo.mysql.typed-session/v1","source_id":source,
            "source_generation":generation,"task_id":context["lease_id"],"l0":l0,
            "previous":previous,"config_digest":config_digest,"schema_digest":schema_digest,
            "typed_tables":capture.specs(),"deadline_unix_ms":deadline});
        let path = out.join("typed-session.json");
        if d.resumed {
            if read_document(&path)? != header {
                return Err(PreviewError::new("c1", "typed_resume_identity"));
            }
        } else { durable_document(&path, &header)?; }
    }
    let mut open=json!({"kind":"open","l0":l0,"l0_pos":capture.initial_position(),
        "source_uuid":capture.live.span.uuid,"connection_config_sha256":sha256_hex(&fs::read(config).map_err(|_|fault())?),
        "config_digest":config_digest,"schema_digest":schema_digest,"admission_config":admission,
        "schema_preimage":schemas,"tables":capture.cfg.tables,
        "export_metadata":export_metadata,
        "table_schemas":native_table_schemas(&capture.cfg.tables,&capture.live.column_names,&capture.live.pk,&capture.live.column_kinds)?,
        "typed_tables":capture.specs(),"typed_delta":delta.is_some(),
        "resume_sequence":delta.as_ref().map(|d|d.sequence).unwrap_or(0),
        "resume_position":delta.as_ref().map(|d|d.position.clone()).unwrap_or(capture.initial_position().to_string()),
        "resume_digest":delta.as_ref().map(|d|d.digest.clone()).unwrap_or(Value::Null)});
    if let Some(reference)=source_registration {open["source_registration"]=reference;}
    send_frame(&open)?;
    let opened=read_worker_reply(&mut input,"stage_open")?;
    let epoch=opened["source_epoch"].as_u64().and_then(|n|u32::try_from(n).ok()).ok_or_else(fault)?;
    if let Some(d) = &delta {
        d.check_open(&opened)?;
    }
    send_frame(&json!({"kind":"stream_started","l0":l0,"position_lineage":lineage}))?;
    read_worker_reply(&mut input,"stream_started")?;
    fs::write(out.join("ready"),b"subscribed").map_err(|_|fault())?;
    let chunk_rows=input_config["c1_chunk_rows"].as_u64().unwrap_or(2500);
    if !(1..=10000).contains(&chunk_rows) {return Err(fault());}
    let mut sequence=delta.as_ref().map(|d|d.sequence+1).unwrap_or(1);
    for name in capture.specs().keys().filter(|_|delta.is_none()) {
        let mut lower=None;
        loop {
            check()?;
            let chunk=capture.snapshot_chunk(name,lower.clone(),chunk_rows,sequence).await?;
            let Some((batch,upper))=chunk else{break};
            send_frame(&json!({"kind":"typed_batch","batch":batch}))?;
            read_typed_reply(&mut input,&batch,&context,epoch)?;
            sequence+=1;lower=Some(upper);
        }
        send_frame(&json!({"kind":"snapshot_end","table":name}))?;
        read_worker_reply(&mut input,"snapshot_end")?;
    }
    check()?;
    if let Some(frame) = pending {
        check()?;
        let batch:Batch=serde_json::from_value(frame["batch"].clone()).map_err(|_|fault())?;
        send_frame(&frame)?;
        read_typed_reply(&mut input,&batch,&context,epoch)?;
        sequence+=1;
    }
    let end_path=out.join("typed-end.json");
    let l_end = if delta.is_some() && end_path.exists() {
        read_document(&end_path)?["l_end"].as_str().ok_or_else(fault)?.to_owned()
    } else {
        let mut probe=connect_cdc(&capture.cfg).await?;
        let (_,_,end)=full_status(&mut probe).await?;
        if delta.is_some() { durable_document(&end_path,&json!({"l_end":end}))?; }
        end
    };
    let end=parse_span(&l_end)?;
    if end.uuid!=capture.live.span.uuid || end.end<capture.live.span.end {return Err(PreviewError::new("c1","lineage"));}
    while capture.live.span.end<end.end {
        check()?;
        let wait=Duration::from_millis(deadline.saturating_sub(now_ms()?).min(30_000));
        let batch=capture.next(sequence,wait).await?;
        let frame=json!({"kind":"typed_batch","batch":batch,
            "source_commit_timestamp":capture.live.source_commit_timestamp});
        if delta.is_some() {
            durable_document(&out.join(format!("typed-{sequence:08}.json")),&frame)?;
        }
        send_frame(&frame)?;
        read_typed_reply(&mut input,&batch,&context,epoch)?;
        sequence+=1;
    }
    send_frame(&json!({"kind":"end","l_end":l_end,"l_end_pos":end.end,"applied_pos":end.end}))?;
    let sealed=read_worker_reply(&mut input,"end")?;
    check()?;
    context["reader_write_attempts"]=json!(reader_write_audit(&capture.cfg,out).await?);
    if let Some(limit)=export_limit {
        let view=export_parts::ExportView{tables:&capture.tables,binding:&capture.binding,
            l0:&capture.l0,uuid:&capture.live.span.uuid};
        let request=export_parts::materialize(&mut input,out,&context,&view,&sealed["target_export"],
            &schemas,&admission,epoch,&l_end,deadline,limit)?;
        write_json(&out.join("native-request.json"),&request)?;
        send_frame(&json!({"kind":"materialized","request":request}))?;
        return Ok(());
    }
    let rows=sealed["rows"].as_array().ok_or_else(fault)?;
    let mut request=crate::c1_materialize::materialize(out,&context,&tables,rows,&l0,&l_end,&lineage,epoch,&schemas,&admission)?;
    request["c1_baseline_format"]=json!("typed-keys/v3");
    write_json(&out.join("native-request.json"),&request)?;
    send_frame(&json!({"kind":"materialized","request":request}))?;
    Ok(())
}

fn quiet_request(context:&Value,observed:&str,config_sha:&str)->Option<Value> {
    let previous=&context["previous"];
    if previous.is_null() || !context["resume"].is_null()
        || previous["l_end"].as_str()!=Some(observed) {return None;}
    Some(json!({"protocol":"oo.jobs.mysql-c1-nochange/v1","source_id":context["source_id"],
        "source_generation":previous["source_generation"],"previous":previous,
        "observed_gtid":observed,"connection_config_sha256":config_sha}))
}

fn validate_checkpoint(checkpoint: &str, executed: &str, purged: &str, uuid: &str) -> Result<(), PreviewError> {
    let point = parse_span(checkpoint)?;
    let current = parse_span(executed)?;
    if point.uuid != uuid || current.uuid != uuid || point.start != 1
        || current.start != 1 || point.end == 0 || point.end > current.end {
        return Err(PreviewError::new("c1", "typed_checkpoint_identity"));
    }
    if !purged.is_empty() {
        let removed = parse_span(purged)?;
        if removed.uuid != uuid || removed.start != 1 || removed.end > point.end {
            return Err(PreviewError::new("c1", "typed_logs_expired_rebaseline_required"));
        }
    }
    Ok(())
}

struct Delta {
    l0: String,
    epoch: u64,
    sequence: i64,
    position: String,
    digest: Value,
    resumed: bool,
}

impl Delta {
    fn parse(context: &Value) -> Result<Option<Self>, PreviewError> {
        let previous = &context["previous"];
        let resume = &context["resume"];
        if previous.is_null() {
            if !resume.is_null() {
                return Err(PreviewError::new("c1", "typed_initial_snapshot_resume_unsupported"));
            }
            return Ok(None);
        }
        let fail = || PreviewError::new("c1", "typed_previous_contract");
        let l0 = previous["l_end"].as_str().ok_or_else(fail)?.to_owned();
        let point = parse_span(&l0)?;
        let epoch = previous["source_epoch"].as_u64().filter(|n| *n > 0 && *n <= u32::MAX as u64).ok_or_else(fail)?;
        if point.end == 0 || point.start != 1
            || previous["measured_id"] != format!("mysql.{}", point.uuid)
            || previous["source_generation"].as_u64().and_then(|n|n.checked_add(1)) != context["source_generation"].as_u64()
            || previous["task_id"].as_str().is_none_or(str::is_empty)
            || context["lease_id"].as_str().is_none_or(str::is_empty)
            || ["config_digest", "schema_digest"].iter().any(|k| !is_digest(&previous[k])) {
            return Err(fail());
        }
        let mut d = Self { l0, epoch, sequence: 0, position: point.end.to_string(), digest: Value::Null, resumed: !resume.is_null() };
        if !resume.is_null() {
            if resume["protocol"] != "oo.mysql.typed-resume/v1"
                || resume["task_id"] != context["lease_id"] || resume["source_epoch"] != epoch
                || resume["l0"] != d.l0 || resume["snapshot_complete"] != true {
                return Err(PreviewError::new("c1", "typed_resume_contract"));
            }
            d.sequence = resume["sequence"].as_i64().filter(|n|*n >= 0 && *n < i64::MAX).ok_or_else(fail)?;
            d.position = resume["position"].as_str().ok_or_else(fail)?.into();
            let pos = d.position.parse::<u64>().map_err(|_|fail())?;
            d.digest = resume["digest"].clone();
            if pos.to_string() != d.position || pos < point.end
                || (d.sequence == 0 && (pos != point.end || !d.digest.is_null()))
                || (d.sequence > 0 && !is_digest(&d.digest)) {
                return Err(PreviewError::new("c1", "typed_resume_checkpoint"));
            }
        }
        Ok(Some(d))
    }

    fn check_open(&self, opened: &Value) -> Result<(), PreviewError> {
        if opened["typed_delta"] != true || opened["source_epoch"] != self.epoch
            || opened["resume_sequence"] != self.sequence
            || opened["resume_position"] != self.position
            || opened["resume_digest"] != self.digest {
            return Err(PreviewError::new("c1", "typed_stage_checkpoint"));
        }
        Ok(())
    }

    fn pending(&self, out: &Path) -> Result<Option<Value>, PreviewError> {
        let fail = || PreviewError::new("c1", "typed_journal_checkpoint");
        let mut paths = fs::read_dir(out).map_err(|_|fail())?
            .filter_map(Result::ok).map(|e|e.path())
            .filter(|p| p.file_name().and_then(|s|s.to_str()).is_some_and(|s|
                s.starts_with("typed-") && s.ends_with(".json") && s != "typed-session.json" && s != "typed-end.json"))
            .collect::<Vec<_>>();
        paths.sort();
        let mut position = parse_span(&self.l0)?.end.to_string();
        let mut found_checkpoint = self.sequence == 0;
        let mut pending = None;
        for (i,path) in paths.iter().enumerate() {
            let frame = read_document(path)?;
            let b:Batch=serde_json::from_value(frame["batch"].clone()).map_err(|_|fail())?;
            b.validate().map_err(|_|fail())?;
            if frame["kind"] != "typed_batch" || b.kind != "cdc" || b.sequence != i as i64+1
                || b.sequence > self.sequence+1 || b.previous_position != position
                || *path != out.join(format!("typed-{:08}.json",b.sequence)) {
                return Err(fail());
            }
            for part in &b.parts {
                let root=out.canonicalize().map_err(|_|fail())?;
                let path=part.path.canonicalize().map_err(|_|fail())?;
                if !path.starts_with(&root) || fs::metadata(&path).map_err(|_|fail())?.len()!=part.bytes
                    || sha256_hex(&fs::read(&path).map_err(|_|fail())?)!=part.sha256 {
                    return Err(PreviewError::new("c1","typed_journal_part"));
                }
            }
            position=b.position.clone();
            if b.sequence == self.sequence {
                if b.position != self.position || b.digest().map_err(|_|fail())? != self.digest {
                    return Err(fail());
                }
                found_checkpoint=true;
            } else if b.sequence > self.sequence { pending=Some(frame); }
        }
        if !found_checkpoint { return Err(fail()); }
        Ok(pending)
    }
}

fn is_digest(v: &Value) -> bool {
    v.as_str().is_some_and(|s|s.len()==64 && s.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

fn read_document(path: &Path) -> Result<Value, PreviewError> {
    serde_json::from_slice(&fs::read(path).map_err(|_|PreviewError::new("c1","typed_journal_missing"))?)
        .map_err(|_|PreviewError::new("c1","typed_journal_corrupt"))
}

fn durable_document(path: &Path, value: &Value) -> Result<(), PreviewError> {
    use std::fs::{File, OpenOptions};
    let fail=||PreviewError::new("c1","typed_journal_write");
    // Final names are immutable; a process killed during write leaves no emitted frame.
    if path.exists() { return Err(fail()); }
    let tmp=path.with_extension("partial");
    let mut f=OpenOptions::new().write(true).create(true).truncate(true).open(&tmp).map_err(|_|fail())?;
    f.write_all(&serde_json::to_vec(value).map_err(|_|fail())?).map_err(|_|fail())?;
    f.sync_all().map_err(|_|fail())?;
    fs::rename(&tmp,path).map_err(|_|fail())?;
    File::open(path.parent().ok_or_else(fail)?).and_then(|d|d.sync_all()).map_err(|_|fail())
}

fn now_ms()->Result<u64,PreviewError> {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .ok().and_then(|t|u64::try_from(t.as_millis()).ok()).ok_or_else(fault)
}

fn read_typed_reply(input:&mut impl BufRead,batch:&Batch,context:&Value,epoch:u32)->Result<(),PreviewError> {
    let ack=read_worker_reply(input,"typed_batch")?;
    validate_typed_ack(&ack["receipt"],batch,context,epoch)
}

fn validate_typed_ack(ack:&Value,batch:&Batch,context:&Value,epoch:u32)->Result<(),PreviewError> {
    if ack["protocol"]!="oo.c1.typed-ack/v3"
        || ack["source"]!=context["source_id"] || ack["task"]!=context["lease_id"]
        || ack["generation"]!=context["lease_generation"] || ack["epoch"]!=epoch
        || ack["sequence"]!=batch.sequence
        || ack["position"]!=batch.position
        || ack["digest"]!=batch.digest().map_err(|_|fault())? {return Err(fault());}
    Ok(())
}

fn fault() -> PreviewError {
    PreviewError::new("c1", "typed_protocol")
}
fn mysql_error(e: mysql_async::Error) -> PreviewError {
    mysql_err(&e)
}
fn part(name: &str, spec: &TableSpec, path: PathBuf) -> Result<Part, PreviewError> {
    let bytes = fs::read(&path).map_err(|_| fault())?;
    let d = oo_pipeline::native_arrow::decode_arrow_stream(&bytes).map_err(|_| fault())?;
    Ok(Part {
        table: name.into(),
        spec: spec.clone(),
        path,
        sha256: sha256_hex(&bytes),
        bytes: bytes.len() as u64,
        rows: d.rows,
    })
}
fn decode_row(
    row: &Seen,
    columns: &[MappedColumn],
) -> Result<Vec<oo_connectors::types::DecodedCell>, PreviewError> {
    columns
        .iter()
        .map(|c| {
            let cell = &row.row_value[&c.plan.name];
            let raw = binlog_raw_cell(cell, c.store)?;
            let raw = values::normalize_binlog_integer(&c.plan.mapping.source_type, raw)?;
            let (cell, reject) = values::decode_cell(c.store, &raw, c.plan.mapping.nullable)?;
            if reject.is_some() {
                return Err(PreviewError::new("value", "typed_reject"));
            }
            Ok(cell)
        })
        .collect()
}

fn binlog_raw_cell(cell: &Value, store: mysql::map::StoreKind) -> Result<RawCell, PreviewError> {
    if store == mysql::map::StoreKind::TimestampTz && cell["type"] == "text"
        && let Some(text) = cell["value"].as_str()
        && !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        // mysql_async's TIMESTAMP2 value is exact decimal Unix seconds, unlike
        // its DATETIME2/calendar representation. Keep the sealed wire unchanged.
        let (seconds, micros) = crate::c1_materialize::temporal_fraction(text)?;
        let seconds = seconds.parse::<u32>().map_err(|_|PreviewError::new("value","binlog_timestamp"))?;
        if seconds == 0 { return Err(PreviewError::new("value","zero_timestamp")); }
        let (year,month,day,hour,minute,second) = civil_from_unix(seconds);
        return Ok(RawCell::Date {year:year as u16,month:month as u8,day:day as u8,
            hour:hour as u8,minute:minute as u8,second:second as u8,micros});
    }
    crate::c1_materialize::raw_cell(cell, store)
}

#[cfg(test)]
mod temporal_wire_tests {
    use super::*;

    #[test]
    fn timestamp2_decimal_seconds_are_not_calendar_text_or_floats() {
        let store = mysql::map::StoreKind::TimestampTz;
        for (text, expected) in [("1.000001", 1_000_001),
            ("1793235723.999999", 1_793_235_723_999_999), ("1793235723.1", 1_793_235_723_100_000)] {
            let wire=json!({"type":"text","value":text});
            let before=wire.clone();
            let raw=binlog_raw_cell(&wire,store).unwrap();
            let (cell,reject)=values::decode_cell(store,&raw,false).unwrap();
            assert!(reject.is_none());
            assert_eq!(cell.arrow,Some(oo_connectors::types::ArrowScalar::TimestampUtc(expected)));
            assert_eq!(wire,before);
        }
        assert!(binlog_raw_cell(&json!({"type":"text","value":"0.000000"}),store).is_err());
        assert!(binlog_raw_cell(&json!({"type":"text","value":"1793235723.0000001"}),store).is_err());
        assert!(matches!(binlog_raw_cell(&json!({"type":"null"}),store).unwrap(),RawCell::Null));
    }
}

#[cfg(test)]
mod continuation_tests {
    use super::*;

    const UUID: &str = "ae5a202f-d16f-11f1-9f19-f601b48e227c";

    fn context() -> Value {
        json!({"source_generation":2,"lease_id":"current-task","previous":{
            "task_id":"previous-task","source_generation":1,"source_epoch":3,
            "l_end":format!("{UUID}:1-100"),"measured_id":format!("mysql.{UUID}"),
            "config_digest":"a".repeat(64),"schema_digest":"b".repeat(64)}})
    }

    fn resume(c: &mut Value, seq: i64, pos: &str, digest: Value) {
        c["resume"]=json!({"protocol":"oo.mysql.typed-resume/v1","task_id":"current-task",
            "source_epoch":3,"l0":format!("{UUID}:1-100"),"snapshot_complete":true,
            "sequence":seq,"position":pos,"digest":digest});
    }

    fn dir() -> PathBuf {
        let p=std::env::temp_dir().join(format!("typed-continuation-{}-{}",std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn frame(seq: i64, prev: u64) -> Value {
        json!({"kind":"typed_batch","batch":{"protocol":PROTOCOL,"sequence":seq,
            "previous_position":prev.to_string(),"position":(prev+1).to_string(),"kind":"cdc","parts":[]}})
    }

    #[test]
    fn typed_checkpoint_refuses_future_foreign_gap_and_expiry() {
        let at=|n|format!("{UUID}:1-{n}");
        assert!(validate_checkpoint(&at(100),&at(120),"",UUID).is_ok());
        assert!(validate_checkpoint(&at(100),&at(120),&at(100),UUID).is_ok());
        assert_eq!(validate_checkpoint(&at(100),&at(120),&at(101),UUID).unwrap_err().fact,
            "typed_logs_expired_rebaseline_required");
        assert!(validate_checkpoint(&at(121),&at(120),"",UUID).is_err());
        assert!(validate_checkpoint(&at(100),&at(120),"","foreign").is_err());
        assert!(validate_checkpoint(&format!("{UUID}:2-100"),&at(120),"",UUID).is_err());
        assert!(validate_checkpoint(&at(100),&format!("{UUID}:1-90:95-120"),"",UUID).is_err());
    }

    #[test]
    fn typed_delta_requires_exact_parent_generation_identity_and_epoch() {
        let c=context();
        let d=Delta::parse(&c).unwrap().unwrap();
        assert_eq!(d.position,"100");
        assert_eq!(d.sequence,0);
        for (key,value) in [("source_epoch",json!(0)),("source_generation",json!(9)),
            ("measured_id",json!("mysql.other")),("config_digest",json!("bad"))] {
            let mut bad=c.clone();bad["previous"][key]=value;
            assert!(Delta::parse(&bad).is_err(),"{key}");
        }
        assert!(Delta::parse(&json!({"resume":{}})).is_err());
    }

    #[test]
    fn typed_resume_refuses_foreign_task_epoch_l0_and_bad_checkpoint() {
        let mut c=context();
        resume(&mut c,0,"100",Value::Null);
        assert!(Delta::parse(&c).is_ok());
        for (key,value) in [("task_id",json!("other")),("source_epoch",json!(4)),
            ("l0",json!(format!("{UUID}:1-99"))),("snapshot_complete",json!(false)),
            ("position",json!("0100")),("position",json!("99")),("digest",json!("c".repeat(64)))] {
            let mut bad=c.clone();bad["resume"][key]=value;
            assert!(Delta::parse(&bad).is_err(),"{key}");
        }
    }

    #[test]
    fn typed_journal_replays_unacked_and_checks_checkpoint_digest() {
        let out=dir();
        let f=frame(1,100);
        durable_document(&out.join("typed-00000001.json"),&f).unwrap();
        let mut c=context();resume(&mut c,0,"100",Value::Null);
        assert_eq!(Delta::parse(&c).unwrap().unwrap().pending(&out).unwrap(),Some(f.clone()));
        let b:Batch=serde_json::from_value(f["batch"].clone()).unwrap();
        resume(&mut c,1,"101",json!(b.digest().unwrap()));
        assert!(Delta::parse(&c).unwrap().unwrap().pending(&out).unwrap().is_none());
        c["resume"]["digest"]=json!("0".repeat(64));
        assert!(Delta::parse(&c).unwrap().unwrap().pending(&out).is_err());
        fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn typed_journal_missing_ahead_and_conflicting_writes_refuse() {
        let out=dir();
        let mut c=context();resume(&mut c,1,"101",json!("c".repeat(64)));
        assert!(Delta::parse(&c).unwrap().unwrap().pending(&out).is_err());
        durable_document(&out.join("typed-00000001.json"),&frame(1,100)).unwrap();
        assert!(durable_document(&out.join("typed-00000001.json"),&frame(1,100)).is_err());
        durable_document(&out.join("typed-00000002.json"),&frame(2,101)).unwrap();
        resume(&mut c,0,"100",Value::Null);
        assert!(Delta::parse(&c).unwrap().unwrap().pending(&out).is_err());
        fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn typed_journal_refuses_changed_part_bytes() {
        let out=dir();
        let path=out.join("cdc-00000001-0.arrow");
        fs::write(&path,b"abc").unwrap();
        let mut f=frame(1,100);
        f["batch"]["parts"]=json!([{"table":"owned.items","spec":{"columns":[
            {"name":"id","oid":20,"typmod":-1,"nullable":false}],"keys":[0]},
            "path":path,"sha256":sha256_hex(b"abc"),"bytes":3,"rows":1}]);
        durable_document(&out.join("typed-00000001.json"),&f).unwrap();
        let mut c=context();resume(&mut c,0,"100",Value::Null);
        assert!(Delta::parse(&c).unwrap().unwrap().pending(&out).is_ok());
        fs::write(path,b"xyz").unwrap();
        assert_eq!(Delta::parse(&c).unwrap().unwrap().pending(&out).unwrap_err().fact,"typed_journal_part");
        fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn typed_open_requires_seed_and_checkpoint_confirmation() {
        let d=Delta::parse(&context()).unwrap().unwrap();
        let good=json!({"typed_delta":true,"source_epoch":3,"resume_sequence":0,"resume_position":"100","resume_digest":null});
        assert!(d.check_open(&good).is_ok());
        for key in ["typed_delta","source_epoch","resume_sequence","resume_position"] {
            let mut bad=good.clone();bad.as_object_mut().unwrap().remove(key);
            assert!(d.check_open(&bad).is_err(),"{key}");
        }
    }

    #[test]
    fn typed_ack_is_bound_to_source_task_lease_epoch_and_batch() {
        let b:Batch=serde_json::from_value(frame(1,100)["batch"].clone()).unwrap();
        let c=json!({"source_id":"source","lease_id":"task","lease_generation":2});
        let ack=json!({"protocol":"oo.c1.typed-ack/v3","source":"source","task":"task",
            "generation":2,"epoch":3,"sequence":1,"position":"101","digest":b.digest().unwrap()});
        assert!(validate_typed_ack(&ack,&b,&c,3).is_ok());
        for key in ["source","task","generation","epoch","sequence","position","digest"] {
            let mut bad=ack.clone();bad[key]=Value::Null;
            assert!(validate_typed_ack(&bad,&b,&c,3).is_err(),"{key}");
        }
    }

    #[test]
    fn typed_quiet_retains_generation_and_never_shortcuts_resume_or_new_events() {
        let c=context();
        let at=format!("{UUID}:1-100");
        let q=quiet_request(&c,&at,&"f".repeat(64)).unwrap();
        assert_eq!(q["source_generation"],1);
        assert_eq!(q["previous"],c["previous"]);
        assert!(quiet_request(&c,&format!("{UUID}:1-101"),&"f".repeat(64)).is_none());
        let mut resumed=c.clone();resume(&mut resumed,0,"100",Value::Null);
        assert!(quiet_request(&resumed,&at,&"f".repeat(64)).is_none());
        assert!(quiet_request(&json!({}),&at,&"f".repeat(64)).is_none());
    }
}
