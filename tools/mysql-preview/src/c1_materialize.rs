//! Materialize only the worker's durable sealed stage, using the existing MySQL
//! type decoder and the ordinary admission sealer. This reader never applies it.
use std::{collections::{BTreeMap, BTreeSet}, fs, path::{Path, PathBuf}, process::Command};
use arrow_ipc::writer::StreamWriter;
use oo_connectors::types::{RunBinding, assemble_batch};
use oo_core::canonical::LogicalType;
use oo_pipeline::c1_parts::{
    C1_BUCKET_BITS, Cell, OP_DELETE, OP_UPSERT, PartEntry, PartKind, PartManifest, StateDigest,
    bucket_key, canonical_row, composed_digest, logical_type_from_name, logical_type_name, read_part,
};
use oo_pipeline::native_arrow::{DecodedStream, NativeCell};
use oo_pipeline::target_installer::sha256_hex;
use serde_json::{Value, json};
use crate::c1_chunked::JournalLine;
use crate::{error::PreviewError, map::{MappedColumn, StoreKind}, values::{self, RawCell}};

type Result<T> = std::result::Result<T, PreviewError>;
fn err() -> PreviewError { PreviewError::new("baseline", "materialization") }
pub(crate) fn write(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec(value).map_err(|_|err())?).map_err(|_|err())
}
/// Type preimage of the baseline: the bytes of `type-preimage.json`, and their digest. Tables are
/// ordered by qualified name, so the producer's `open` (which sends the digest) and materialize
/// (which writes the file) compute the same bytes.
pub(crate) fn type_preimage<'a>(tables: impl IntoIterator<Item = (&'a str, &'a str, &'a [V2Column])>) -> Result<(Vec<u8>, String)> {
    let mut order: Vec<(&str, &str, &[V2Column])> = tables.into_iter().collect();
    order.sort_by(|a, b| a.0.cmp(b.0));
    let mut rows = Vec::new();
    for (_, table, columns) in order {
        rows.extend(columns.iter().map(|column| column.type_json(table)));
    }
    let bytes = serde_json::to_vec(&json!(rows)).map_err(|_| err())?;
    let digest = sha256_hex(&bytes);
    Ok((bytes, digest))
}

/// Digest of one table's schema preimage (the bytes of its `schema-preimage.json`). The producer
/// computes it at `open` (the worker binds it per table) and materialize writes the same bytes.
pub(crate) fn table_schema_digest(table: &str, columns: &[V2Column]) -> String {
    let columns: Vec<Value> = columns.iter().map(V2Column::schema_json).collect();
    let doc = json!({"table": table, "columns": columns});
    sha256_hex(&serde_json::to_vec(&json!([doc])).unwrap_or_default())
}

/// Input digest of one baseline run. Every field is known at `open`: the producer sends this value
/// in the open frame, the worker binds it, and the admission and the final load recompute it from
/// the same inputs (L0, the admission config digest, the schema digest, the type digest).
pub(crate) fn input_digest(l0: &str, config_digest: &str, schema_digest: &str, type_digest: &str) -> String {
    sha256_hex(format!("oo.c1.input/v3\n{l0}\n{config_digest}\n{schema_digest}\n{type_digest}\n").as_bytes())
}

pub(crate) fn admission_config(context: &Value, tables: &[String]) -> Value {
    json!({"protocol":"oo.source-admission.config/v1","target_id":context["source_id"],"source_id":context["source_id"],
        "secret_ref":context["secret_ref"].as_str().unwrap_or("secret://mysql-native"),"compat_mode":"mysql-native",
        "permissions":tables.iter().map(|table| { let (schema,name)=table.split_once('.').unwrap_or(("",""));
            json!({"name":"SELECT","status":"known","granted":true,"scope":{"database":schema,"table":name}}) }).collect::<Vec<_>>(),
        "r1w":null,"budget":{"unit":"Rows","hard_limit":100000,"confidence":"unknown"},
        "write_options":{"signal_table":false,"heartbeat_write":false,"create_stream":false,"create_trigger":false},
        "debezium":{"signal_data_collection":null,"heartbeat_action_query":null,"snapshot_mode":"never","create_stream":false,"create_trigger":false},
        "requested_write_class":"R0"})
}
pub(crate) fn schema_columns(columns: &[MappedColumn]) -> Result<Vec<Value>> {
    columns.iter().map(|column| {
        let (kind,precision,scale)=crate::capture::arrow_description(column.plan.arrow.as_ref().ok_or_else(err)?)?;
        Ok(json!({"name":column.plan.name,"arrow_type":kind,"nullable":column.plan.mapping.nullable,"precision":precision,
            "scale":scale,"key":column.key,"document":column.oid==114,"char_length":if column.oid==1042 {Some(column.typmod-4)} else {None}}))
    }).collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn materialize(out: &Path, context: &Value, tables: &[(String,String,Vec<MappedColumn>)],
    rows: &[Value], l0: &str, l_end: &str, lineage: &str, epoch: u32, schemas: &Value, config: &Value) -> Result<Value> {
    let source=context["source_id"].as_str().ok_or_else(err)?;
    let generation=context["source_generation"].as_u64().ok_or_else(err)?;
    let driver=std::env::current_exe().map_err(|_|err())?;
    let admission=context["admission_binary"].as_str().ok_or_else(err)?;
    if sha256_hex(&fs::read(admission).map_err(|_|err())?) != context["admission_sha256"] {return Err(err());}
    let driver_sha=sha256_hex(&fs::read(&driver).map_err(|_|err())?);
    let config_bytes=serde_json::to_vec(config).map_err(|_|err())?;
    let schema_bytes=serde_json::to_vec(schemas).map_err(|_|err())?;
    let config_digest=sha256_hex(&config_bytes); let schema_digest=sha256_hex(&schema_bytes);
    write(&out.join("admission-config.json"),config)?; write(&out.join("schema-preimage.json"),schemas)?;
    let mut types=Vec::new();
    for (schema,table,columns) in tables {
        let _=schema;
        for (column,mut field) in columns.iter().zip(schema_columns(columns)?) {
            field["table"]=json!(table);field["source_type"]=json!(column.plan.mapping.source_type);
            field["target_oid"]=json!(column.oid);field["target_typmod"]=json!(column.typmod);field["caveat"]=json!(column.caveat);
            let map=field.as_object_mut().ok_or_else(err)?;map.remove("nullable");map.remove("key");types.push(field);
        }
    }
    write(&out.join("type-preimage.json"),&json!(types))?;
    let mut native_requests=Vec::new();let mut entries=Vec::new();let mut capture_tables=Vec::new();let mut scope_tables=Vec::new();
    for (index,(schema,table,columns)) in tables.iter().enumerate() {
        let selected=rows.iter().filter(|r|r["schema"]==*schema && r["table"]==*table && r["deleted"]==false).collect::<Vec<_>>();
        let mut decoded=Vec::with_capacity(selected.len());
        let mut rejects=Vec::new();
        let mut keys=BTreeSet::new();
        for (row_index,row) in selected.iter().enumerate() {
            if !keys.insert(serde_json::to_vec(&row["key"]).map_err(|_|err())?) {return Err(err());}
            let mut cells=Vec::with_capacity(columns.len());
            for col in columns {
                let raw=raw_cell(&row["value"][&col.plan.name],col.store)?;
                let (cell,reject)=values::decode_cell(col.store,&raw,col.plan.mapping.nullable)?;
                if let Some(note)=reject {
                    let refused=values::reject_for(&note);
                    rejects.push(json!({"attnum":col.plan.attnum,"column":col.plan.name,"key_digest":"","reason":refused.reason,"row":row_index,"table":table,"value_len":refused.value_len}));
                }
                cells.push(cell);
            }
            decoded.push(cells);
        }
        let binding=RunBinding { source_id:source.to_owned(),schema:schema.clone(),table:table.clone(),run_generation:generation,input_digest:[0;32] };
        let keys=columns.iter().filter(|c|c.key).map(|c|c.plan.attnum).collect::<Vec<_>>();
        let typed=assemble_batch(&binding,columns.iter().map(|c|c.plan.clone()).collect(),&keys,decoded).map_err(|_|err())?;
        for reject in &mut rejects {
            let row=typed.rows.get(reject["row"].as_u64().ok_or_else(err)? as usize).ok_or_else(err)?;
            let digest=row.key_digest.ok_or_else(err)?;
            reject["key_digest"]=json!(digest.iter().map(|b|format!("{b:02x}")).collect::<String>());
        }
        let batch=typed.record_batch().map_err(|_|err())?;
        let path=out.join(format!("table-{index}.arrow"));
        let mut writer=StreamWriter::try_new(fs::File::create(&path).map_err(|_|err())?,&batch.schema()).map_err(|_|err())?;
        writer.write(&batch).map_err(|_|err())?;writer.finish().map_err(|_|err())?;
        let sha=sha256_hex(&fs::read(&path).map_err(|_|err())?);
        let schema_sha=sha256_hex(&serde_json::to_vec(&json!([{ "table":table,"columns":schema_columns(columns)? }])).map_err(|_|err())?);
        entries.push(json!({"schema":schema,"table":table,"source_generation":generation,"schema_sha256":schema_sha,
            "snapshot_arrow":{"path":path,"sha256":sha},"transactions":[]}));
        scope_tables.push(json!({"schema":schema,"table":table,"row_count":selected.len(),"arrow_sha256":sha,"schema_sha256":schema_sha}));
        capture_tables.push(json!({"name":table,"schema":schema,"arrow_path":path,"arrow_sha256":sha,"row_count":selected.len(),
            "route":"C1","consistency":"T","delete_mode":"native","cursor_safety":"no_cursor","quality_rejects":rejects}));
    }
    let identity=json!({"engine":"mysql","measured_id":lineage,"shard_id":context["target"]["shard"],"owner_token":context["target"]["owner"]});
    let mut scope=json!({"protocol":"oo.source-admission.c1-baseline-scope/v1","engine":"mysql","lineage":lineage,
        "l0":l0,"l_end":l_end,"complete":true,"read_fence_digest":sha256_hex(&serde_json::to_vec(rows).map_err(|_|err())?),
        "config_digest":config_digest,"schema_digest":schema_digest,"consistency_grade":"T","cross_table_consistent":true,
        "source_identity":identity,"tables":scope_tables});
    let mut envelope=json!({"protocol":"oo.source-admission.c1-capture-envelope/v1","baseline_scope":scope,"entries":entries,"capture_digest":"0".repeat(64)});
    let capture_digest=sha256_hex(&serde_json::to_vec(&envelope).map_err(|_|err())?);scope["capture_digest"]=json!(capture_digest);
    // Admission binds source rows and the global manifest before registry refs exist.
    envelope["capture_digest"]=json!(capture_digest);
    let snapshot=json!({"kind":"mysql_cdc_first","value":l_end});
    let capturer=json!({"protocol":"oo.mysql.capture-tick/v1","source_id":source,"source_generation":generation,
        "run_id":context["target"]["owner"],"database":tables.first().ok_or_else(err)?.0,"source_preflight":context["source_preflight"],"snapshot":snapshot,"tables":capture_tables,"product_reader_write_attempts":context["reader_write_attempts"],"snapshot_token":false,
        "cross_table_consistency":"T","baseline_scope":scope,"capture_envelope":envelope});
    write(&out.join("capturer.json"),&capturer)?;
    for entry in &mut entries {
        let table=entry["table"].as_str().ok_or_else(err)?.to_owned();let schema=entry["schema"].as_str().ok_or_else(err)?.to_owned();
        let dest=out.join(&table);fs::create_dir(&dest).map_err(|_|err())?;
        let table_schema=schemas.as_array().ok_or_else(err)?.iter().find(|s|s["table"]==table).ok_or_else(err)?;
        write(&dest.join("schema-preimage.json"),&json!([table_schema]))?;
        let witness=json!({"protocol":"oo.source-admission.capture-witness/v1","source_engine":"mysql","source_id":source,
            "source_generation":generation,"snapshot":snapshot,"run_id":context["target"]["owner"],"logical_schema":schema,"logical_table":table,
            "capturer_path":out.join("capturer.json"),"arrow_path":entry["snapshot_arrow"]["path"],"driver_path":driver,
            "schema_preimage_path":dest.join("schema-preimage.json"),"type_preimage_path":out.join("type-preimage.json")});
        write(&dest.join("witness.json"),&witness)?;
        let report=json!({"protocol":"oo.source-admission.report/v1","target_id":source,"source_id":source,"secret_ref":config["secret_ref"],
            "compat_mode":"mysql-native","permissions":config["permissions"],"r1w":null,
            "driver":{"kind":"native","name":"mysql_async","version":"0.36.1","digest":driver_sha},"license_flags":["GPL-2.0"],
            "capabilities":[{"name":"Cdc","cursor":null,"feed":null,"cdc":"Binlog","token_order":"Total","budget_unit":"Rows","level":"experimental","write_class":"R0","consistency":"T"}],
            "capture_methods":[{"method":"C1","write_class":"R0","consistency":"T","token_order":"Total","retention_window":format!("{}s",context["source_preflight"]["binlog_expire_logs_seconds"].as_u64().ok_or_else(err)?),"cursor":null,"feed":null,"cdc":"Binlog","delete_detection":"native"}],
            "cost_model":{"unit":"Rows","confidence":"unknown","estimate":"bounded owned fixture"},"capture_witness":dest.join("witness.json")});
        write(&dest.join("report.json"),&report)?;
        let output=Command::new(admission).args(["seal-native","--report"]).arg(dest.join("report.json"))
            .arg("--config").arg(out.join("admission-config.json")).arg("--out").arg(dest.join("sealed")).output().map_err(|_|err())?;
        fs::write(dest.join("seal.stdout"),&output.stdout).map_err(|_|err())?;fs::write(dest.join("seal.stderr"),&output.stderr).map_err(|_|err())?;
        fs::write(dest.join("seal.exit"),output.status.code().unwrap_or(-1).to_string()).map_err(|_|err())?;
        if !output.status.success() { return Err(err()); }
        let registry_path=dest.join("sealed/registry.json");let registry=fs::read(&registry_path).map_err(|_|err())?;
        let registry_value:Value=serde_json::from_slice(&registry).map_err(|_|err())?;
        let b=&registry_value["bindings"][0];
        let columns=b["fields"].as_array().ok_or_else(err)?.iter().map(|f|{let mut f=f.clone();f["logical_name"]=f["name"].clone();f}).collect::<Vec<_>>();
        let mut request=json!({"protocol":"oo.pipeline.native-update/v1","source_engine":"mysql","source_id":source,"source_generation":generation,
            "parent_source_generation":if generation==1 {None} else {Some(generation-1)},"logical_schema":schema,"logical_table":table,
            "batch_id":format!("c1-{}-{}",context["binding"].as_str().ok_or_else(err)?,context["pin"]),"run_id":b["run_id"],
            "capture_method":"c1_cdc","delete_detection":"native","consistency_grade":"T","cross_table_consistent":true,
            "position_codec":"mysql-gtid/v1","position_lineage":lineage,"applied_pos":l_end.rsplit('-').next().ok_or_else(err)?,
            "full_snapshot":true,"columns":columns,"key_columns":columns.iter().filter(|f|f["key"]==true).map(|f|f["name"].clone()).collect::<Vec<_>>(),
            "row_positions":rows.iter().filter(|r|r["schema"]==schema && r["table"]==table && r["deleted"]==false).map(|r|r["applied_pos"].clone()).collect::<Vec<_>>(),
            "tombstones":[],"arrow_path":entry["snapshot_arrow"]["path"],"max_arrow_bytes":33554432,
            "admission_path":b["admission_receipt_path"],"snapshot":b["snapshot"],"audit_digest":b["capture_receipt_sha256"],"target":context["target"],"mirror":false});
        for key in ["config_digest","schema_digest","type_digest","input_digest"] {request[key]=b[key].clone();}
        native_requests.push(request);entry["admission_registry_path"]=json!(registry_path);entry["admission_registry_sha256"]=json!(sha256_hex(&registry));
    }
    // Keep the authenticated envelope entries equal to the exact request. Only
    // derived registry references are omitted from its capture digest projection.
    // Native loader requests are separate from the source capture envelope.
    let mut result=json!({"protocol":"oo.pipeline.native-c1-baseline/v1","source_id":source,"binding":context["binding"],"pin":context["pin"],
        "source_generation":generation,"capture_method":"c1_cdc","target_database":context["target"]["database"],"source_epoch":epoch,
        "source_identity":identity,"baseline_scope":scope,"entries":entries,"native_requests":native_requests,"capture_envelope_path":out.join("capture-envelope.json")});
    envelope["entries"]=result["entries"].clone();write(&out.join("capture-envelope.json"),&envelope)?;
    result["capture_envelope_sha256"]=json!(sha256_hex(&fs::read(out.join("capture-envelope.json")).map_err(|_|err())?));
    write(&out.join("native-request.json"),&result)?;Ok(result)
}

// ---------------------------------------------------------------------------
// Chunked baseline v2 (DESIGN §3.6, §3.8, §9 D1-D2; T10).
//
// The composed expectation is computed in three streaming passes over the
// parts, one part in memory at a time (`c1_parts::read_part`):
//   1. `note_cdc`   over CDC rows: newest event per key. Memory: one entry per
//                   CDC-touched key, bounded by `max_cdc_keys`.
//   2. `add_fill`   over fill rows: a row whose key was touched by CDC is
//                   skipped, every other row is added to the digest.
//   3. `add_image`  over CDC rows again: the final upsert image of each touched
//                   key is added.
// Under the single-write rules (§3.1) the final state is: untouched fill rows
// plus, for each touched key, its image if the newest event is an upsert and
// nothing otherwise. No subtraction is needed, so `c1_parts` is unchanged.
// ---------------------------------------------------------------------------

/// Default row budget per table (DESIGN §3.9 `max_table_rows`).
pub(crate) const DEFAULT_MAX_TABLE_ROWS: u64 = 50_000_000;
/// Default bound on CDC-touched keys held in memory (DESIGN §3.9).
pub(crate) const DEFAULT_MAX_CDC_KEYS: usize = 20_000_000;
/// Witness protocol for admission v2.
pub(crate) const WITNESS_V2: &str = "oo.source-admission.capture-witness/v2";
/// CDC batch list protocol, read by admission v2.
pub(crate) const CDC_BATCHES_V2: &str = "oo.source-admission.c1-cdc-batches/v2";
/// Baseline scope protocol carried in the capturer.
pub(crate) const SCOPE_V2: &str = "oo.source-admission.c1-baseline-scope/v2";
/// Final-load request protocol for T8.
pub(crate) const REQUEST_V2: &str = "oo.pipeline.native-c1-baseline/v2";
/// Composition rule name recorded in every v2 document.
pub(crate) const COMPOSITION_RULE: &str = "c1-compose/v2";
const BATCH_PREFIX: &str = "oo.c1.batch/v2\n";
const CHAIN_PREFIX: &str = "oo.c1.cdc-chain/v2\n";

fn v2_fail<T>(fact: &'static str) -> Result<T> {
    Err(PreviewError::new("baseline_v2", fact))
}

/// Producer-side facts of one column, for manifests and preimages.
#[derive(Clone, Debug)]
pub(crate) struct V2Column {
    /// Target column name.
    pub name: String,
    /// Source attribute number.
    pub attnum: i32,
    /// Source column type text.
    pub source_type: String,
    /// Nullable flag.
    pub nullable: bool,
    /// Primary-key flag.
    pub key: bool,
    /// Target type oid. Used for the target typing and for parsing text, never for the canonical type.
    pub oid: i64,
    /// Target type modifier.
    pub typmod: i32,
    /// Caveat text, if any.
    pub caveat: Option<String>,
    /// Arrow kind, precision and scale.
    pub arrow: (String, Option<i32>, Option<i32>),
    /// Canonical logical type: the source mapping's `LogicalType` (DESIGN D7). A MySQL
    /// JSON column is `Text`, not `Json`.
    pub logical: LogicalType,
}

impl V2Column {
    /// Facts of a mapped MySQL column.
    pub(crate) fn from_mapped(column: &MappedColumn) -> Result<Self> {
        let (kind, precision, scale) = crate::capture::arrow_description(column.plan.arrow.as_ref().ok_or_else(err)?)?;
        Ok(Self {
            name: column.plan.name.clone(),
            attnum: column.plan.attnum,
            source_type: column.plan.mapping.source_type.clone(),
            nullable: column.plan.mapping.nullable,
            key: column.key,
            oid: column.oid,
            typmod: column.typmod,
            caveat: column.caveat.map(str::to_owned),
            arrow: (kind.to_owned(), precision, scale),
            logical: column.plan.logical.clone().ok_or_else(err)?,
        })
    }

    /// The facts the producer writes into `parts-manifest.json`, and reads back.
    pub(crate) fn to_facts(&self) -> Result<Value> {
        Ok(json!({"name": self.name, "attnum": self.attnum, "source_type": self.source_type,
            "nullable": self.nullable, "key": self.key, "oid": self.oid, "typmod": self.typmod,
            "caveat": self.caveat, "arrow": {"kind": self.arrow.0, "precision": self.arrow.1, "scale": self.arrow.2},
            "logical": logical_type_name(&self.logical).map_err(lift_parts)?}))
    }

    /// Inverse of [`V2Column::to_facts`]. Any missing or mistyped field is refused.
    pub(crate) fn from_facts(value: &Value) -> Result<Self> {
        let text = |key: &str| value[key].as_str().map(str::to_owned).ok_or_else(err);
        let arrow = &value["arrow"];
        Ok(Self {
            name: text("name")?,
            attnum: value["attnum"].as_i64().and_then(|n| i32::try_from(n).ok()).ok_or_else(err)?,
            source_type: text("source_type")?,
            nullable: value["nullable"].as_bool().ok_or_else(err)?,
            key: value["key"].as_bool().ok_or_else(err)?,
            oid: value["oid"].as_i64().ok_or_else(err)?,
            typmod: value["typmod"].as_i64().and_then(|n| i32::try_from(n).ok()).ok_or_else(err)?,
            caveat: value["caveat"].as_str().map(str::to_owned),
            arrow: (
                arrow["kind"].as_str().map(str::to_owned).ok_or_else(err)?,
                arrow["precision"].as_i64().and_then(|n| i32::try_from(n).ok()),
                arrow["scale"].as_i64().and_then(|n| i32::try_from(n).ok()),
            ),
            logical: logical_type_from_name(value["logical"].as_str().ok_or_else(err)?).map_err(lift_parts)?,
        })
    }

    fn schema_json(&self) -> Value {
        json!({"name":self.name,"arrow_type":self.arrow.0,"nullable":self.nullable,"precision":self.arrow.1,
            "scale":self.arrow.2,"key":self.key,"document":self.oid==114,
            "char_length":if self.oid==1042 {Some(self.typmod-4)} else {None}})
    }

    fn type_json(&self, table: &str) -> Value {
        let mut field = self.schema_json();
        field["table"] = json!(table);
        field["source_type"] = json!(self.source_type);
        field["target_oid"] = json!(self.oid);
        field["target_typmod"] = json!(self.typmod);
        field["caveat"] = json!(self.caveat);
        if let Some(map) = field.as_object_mut() {
            map.remove("nullable");
            map.remove("key");
        }
        field
    }
}

/// One table of a v2 baseline: columns, its parts directory, and its parts.
#[derive(Clone, Debug)]
pub(crate) struct V2Table {
    /// Source schema.
    pub schema: String,
    /// Source table.
    pub table: String,
    /// Columns in target order.
    pub columns: Vec<V2Column>,
    /// Directory holding the part files named by `parts`.
    pub parts_dir: PathBuf,
    /// Fill and CDC parts of this table.
    pub parts: Vec<PartEntry>,
}

/// One CDC batch: the transactions `first_pos..=last_pos` and the CDC parts it
/// carries, as `(index into the table list, cdc part_no)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct V2Batch {
    /// 1-based batch number.
    pub batch_no: u32,
    /// First transaction position.
    pub first_pos: u64,
    /// Last transaction position.
    pub last_pos: u64,
    /// CDC parts of this batch.
    pub refs: Vec<(usize, u32)>,
}

/// Row budget and CDC memory bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct V2Policy {
    /// Admission row budget (`hard_limit`) per table.
    pub max_table_rows: u64,
    /// Maximum number of CDC-touched keys per table.
    pub max_cdc_keys: usize,
}

impl Default for V2Policy {
    fn default() -> Self {
        Self {
            max_table_rows: DEFAULT_MAX_TABLE_ROWS,
            max_cdc_keys: DEFAULT_MAX_CDC_KEYS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Touched {
    pos: u64,
    seq: u64,
    upsert: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pass {
    Notes,
    Fills,
    Images,
}

/// Streaming composition of one table. See the section comment.
pub(crate) struct Composer {
    types: Vec<LogicalType>,
    key: usize,
    max_keys: usize,
    touched: BTreeMap<i128, Touched>,
    digest: StateDigest,
    pass: Pass,
    seq: u64,
    noted: u64,
    last_fill: Option<i128>,
}

impl Composer {
    /// Composer for one table. The key column must be an integer.
    pub(crate) fn new(types: Vec<LogicalType>, key: usize, max_keys: usize) -> Result<Self> {
        if !key_type_ok(types.get(key)) {
            return v2_fail("key_type");
        }
        Ok(Self {
            types,
            key,
            max_keys,
            touched: BTreeMap::new(),
            digest: StateDigest::new(C1_BUCKET_BITS).map_err(lift_parts)?,
            pass: Pass::Notes,
            seq: 0,
            noted: 0,
            last_fill: None,
        })
    }

    fn key_of(&self, row: &[Cell]) -> Result<i128> {
        if row.len() != self.types.len() {
            return v2_fail("row_width");
        }
        bucket_key(&row[self.key]).map_err(lift_parts)
    }

    /// Pass 1. Record one CDC row. The newest position per key wins. Two rows of
    /// one key at the same position are rejected, as the model rejects them.
    pub(crate) fn note_cdc(&mut self, key: i128, pos: u64, upsert: bool) -> Result<()> {
        if self.pass != Pass::Notes || pos == 0 {
            return v2_fail("cdc_pos");
        }
        if !self.touched.contains_key(&key) && self.touched.len() >= self.max_keys {
            return v2_fail("cdc_keys_exceed");
        }
        match self.touched.get(&key).copied() {
            Some(old) if old.pos == pos => return v2_fail("cdc_tie"),
            Some(old) if old.pos > pos => {}
            _ => {
                self.touched.insert(
                    key,
                    Touched {
                        pos,
                        seq: self.noted,
                        upsert,
                    },
                );
            }
        }
        self.noted += 1;
        Ok(())
    }

    /// Start pass 2.
    pub(crate) fn begin_fills(&mut self) -> Result<()> {
        if self.pass != Pass::Notes {
            return v2_fail("pass");
        }
        self.pass = Pass::Fills;
        Ok(())
    }

    /// Pass 2. Add one fill row unless CDC touched its key. Returns its key. Keys
    /// must ascend across all fill parts of the table.
    pub(crate) fn add_fill(&mut self, row: &[Cell]) -> Result<i128> {
        if self.pass != Pass::Fills {
            return v2_fail("pass");
        }
        let key = self.key_of(row)?;
        if self.last_fill.is_some_and(|prev| key <= prev) {
            return v2_fail("fill_order");
        }
        self.last_fill = Some(key);
        if self.touched.contains_key(&key) {
            return Ok(key);
        }
        let bytes = canonical_row(&self.types, row).map_err(lift_parts)?;
        self.digest.add_row(key, &bytes).map_err(lift_parts)?;
        Ok(key)
    }

    /// Start pass 3. The sequence restarts so rows match pass 1 by order.
    pub(crate) fn begin_images(&mut self) -> Result<()> {
        if self.pass != Pass::Fills {
            return v2_fail("pass");
        }
        self.pass = Pass::Images;
        self.seq = 0;
        Ok(())
    }

    /// Pass 3. Visit the CDC rows in pass 1 order. `row` is `Some` for an upsert
    /// and `None` for a delete. Only the newest event of a key contributes.
    pub(crate) fn add_image(&mut self, key: i128, row: Option<&[Cell]>) -> Result<()> {
        if self.pass != Pass::Images {
            return v2_fail("pass");
        }
        let seq = self.seq;
        self.seq += 1;
        if seq >= self.noted {
            return v2_fail("cdc_pass");
        }
        let Some(touched) = self.touched.get(&key).copied() else {
            return v2_fail("cdc_pass");
        };
        if touched.seq != seq || !touched.upsert {
            return Ok(());
        }
        let row = row.ok_or_else(|| PreviewError::new("baseline_v2", "cdc_image"))?;
        if self.key_of(row)? != key {
            return v2_fail("cdc_pass");
        }
        let bytes = canonical_row(&self.types, row).map_err(lift_parts)?;
        self.digest.add_row(key, &bytes).map_err(lift_parts)
    }

    /// Row count and digest. Every pass must have run over the same CDC rows.
    pub(crate) fn finish(self) -> Result<(u64, StateDigest)> {
        if self.pass != Pass::Images || self.seq != self.noted {
            return v2_fail("cdc_pass");
        }
        Ok((self.digest.rows(), self.digest))
    }
}

/// A single-integer key: `Int`, or the scale-0 decimal of `BIGINT UNSIGNED` (F3).
pub(crate) fn key_type_ok(ty: Option<&LogicalType>) -> bool {
    matches!(ty, Some(LogicalType::Int) | Some(LogicalType::Decimal { scale: 0, .. }))
}

fn hex32(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn lift_parts(err: oo_pipeline::c1_parts::C1PartsError) -> PreviewError {
    PreviewError::new("baseline_v2", err.reason)
}

/// Canonical cell of one Arrow value under its target logical type.
pub(crate) fn cell_of(ty: &LogicalType, cell: &NativeCell) -> Result<Cell> {
    Ok(match (ty, cell) {
        (_, NativeCell::Null) => Cell::Null,
        (LogicalType::Bool, NativeCell::Bool(value)) => Cell::Bool(*value),
        (LogicalType::Int, NativeCell::Int { value, .. }) => Cell::Int(i128::from(*value)),
        (LogicalType::Int, NativeCell::UInt64(value)) => Cell::Int(i128::from(*value)),
        (LogicalType::Int, NativeCell::UInt8(value)) => Cell::Int(i128::from(*value)),
        (LogicalType::Decimal { .. }, NativeCell::Decimal { coefficient, scale, .. }) => {
            Cell::Decimal(decimal_text(*coefficient, *scale)?)
        }
        (LogicalType::Float64, NativeCell::Float64(bits)) => Cell::Float64(*bits),
        (LogicalType::Text | LogicalType::Json, NativeCell::Utf8(text) | NativeCell::Json(text)) => {
            Cell::Text(text.clone())
        }
        (LogicalType::Bytes, NativeCell::Binary(bytes)) => Cell::Bytes(bytes.clone()),
        (LogicalType::Date, NativeCell::Date32(days)) => {
            let (year, month, day) = civil_from_days(i64::from(*days));
            Cell::Date {
                year: i32::try_from(year).map_err(|_| PreviewError::new("baseline_v2", "date"))?,
                month: month as u8,
                day: day as u8,
            }
        }
        (LogicalType::DurationMicros, NativeCell::Int { value, .. }) => Cell::Micros(*value),
        (
            LogicalType::DurationMicros | LogicalType::TimestampNaive | LogicalType::TimestampTz,
            NativeCell::TimestampMicros(value) | NativeCell::TimestampMicrosUtc { value, .. },
        ) => Cell::Micros(*value),
        (
            LogicalType::TimestampNaive | LogicalType::TimestampTz,
            NativeCell::TimestampNanos { value, .. },
        ) if value % 1000 == 0 => Cell::Micros(value / 1000),
        _ => return v2_fail("cell_type"),
    })
}

/// Plain decimal text of an unscaled coefficient. The composed encoding
/// normalizes it to the column scale.
fn decimal_text(coefficient: i128, scale: i8) -> Result<String> {
    if scale < 0 {
        return v2_fail("decimal_scale");
    }
    let negative = coefficient < 0;
    let digits = coefficient.unsigned_abs().to_string();
    let scale = scale as usize;
    let padded = format!("{digits:0>width$}", width = scale + 1);
    let (whole, fraction) = padded.split_at(padded.len() - scale);
    let sign = if negative { "-" } else { "" };
    Ok(if scale == 0 {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{fraction}")
    })
}

/// Proleptic Gregorian date of days since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Row `row` of a decoded stream, first `width` columns, as cells.
fn data_row(stream: &DecodedStream, row: usize, types: &[LogicalType]) -> Result<Vec<Cell>> {
    types
        .iter()
        .enumerate()
        .map(|(column, ty)| {
            let cell = stream
                .columns
                .get(column)
                .and_then(|values| values.get(row))
                .ok_or_else(|| PreviewError::new("baseline_v2", "part_row"))?;
            cell_of(ty, cell)
        })
        .collect()
}

/// Position and operation of a CDC row. The two columns after `width` are
/// `_oo_pos` and `_oo_op`.
fn cdc_facts(stream: &DecodedStream, row: usize, width: usize) -> Result<(u64, bool)> {
    let pos = match stream.columns.get(width).and_then(|values| values.get(row)) {
        Some(NativeCell::Decimal { coefficient, .. }) => {
            u64::try_from(*coefficient).map_err(|_| PreviewError::new("baseline_v2", "cdc_pos"))?
        }
        _ => return v2_fail("cdc_pos"),
    };
    let upsert = match stream.columns.get(width + 1).and_then(|values| values.get(row)) {
        Some(NativeCell::Utf8(op)) if op.as_str() == OP_UPSERT => true,
        Some(NativeCell::Utf8(op)) if op.as_str() == OP_DELETE => false,
        _ => return v2_fail("cdc_op"),
    };
    Ok((pos, upsert))
}

/// Entries of one kind in part order.
fn entries_of(manifest: &PartManifest, kind: PartKind) -> Vec<PartEntry> {
    manifest
        .parts()
        .iter()
        .filter(|entry| entry.kind == kind)
        .cloned()
        .collect()
}

/// Composed state of one table, with its row count.
pub(crate) struct TableComposition {
    /// Expected row count.
    pub rows: u64,
    /// Expected state digest of the table.
    pub digest: StateDigest,
}

/// Expected state of one table, streamed from its parts. Each CDC part must sit
/// inside its batch range (`ranges` maps CDC `part_no` to `(first, last)`).
pub(crate) fn compose_table(
    dir: &Path,
    manifest: &PartManifest,
    types: &[LogicalType],
    key: usize,
    ranges: &BTreeMap<u32, (u64, u64)>,
    max_keys: usize,
) -> Result<TableComposition> {
    let fills = entries_of(manifest, PartKind::Fill);
    let cdcs = entries_of(manifest, PartKind::Cdc);
    if fills.is_empty() {
        return v2_fail("fill_missing");
    }
    let width = types.len();
    let mut composer = Composer::new(types.to_vec(), key, max_keys)?;
    for entry in &cdcs {
        let &(lo, hi) = ranges.get(&entry.part_no).ok_or_else(|| PreviewError::new("baseline_v2", "cdc_batch_missing"))?;
        let stream = read_part(dir, entry).map_err(lift_parts)?;
        for row in 0..stream.rows {
            let cells = data_row(&stream, row, types)?;
            let (pos, upsert) = cdc_facts(&stream, row, width)?;
            if pos < lo || pos > hi {
                return v2_fail("cdc_pos_range");
            }
            let row_key = composer.key_of(&cells)?;
            composer.note_cdc(row_key, pos, upsert)?;
        }
    }
    composer.begin_fills()?;
    for entry in &fills {
        let stream = read_part(dir, entry).map_err(lift_parts)?;
        for row in 0..stream.rows {
            let cells = data_row(&stream, row, types)?;
            let row_key = composer.add_fill(&cells)?;
            let inside = entry.key_lo.is_some_and(|lo| row_key >= lo)
                && entry.key_hi.is_some_and(|hi| row_key <= hi);
            if !inside {
                return v2_fail("chunk_range");
            }
        }
    }
    composer.begin_images()?;
    for entry in &cdcs {
        let stream = read_part(dir, entry).map_err(lift_parts)?;
        for row in 0..stream.rows {
            let cells = data_row(&stream, row, types)?;
            let (_, upsert) = cdc_facts(&stream, row, width)?;
            let row_key = composer.key_of(&cells)?;
            composer.add_image(row_key, upsert.then_some(cells.as_slice()))?;
        }
    }
    let (rows, digest) = composer.finish()?;
    Ok(TableComposition { rows, digest })
}

/// One CDC part reference inside a batch, as the batch list records it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BatchRef {
    /// Source schema.
    pub schema: String,
    /// Source table.
    pub table: String,
    /// CDC part number of the table.
    pub part_no: u32,
    /// Part file SHA-256.
    pub sha256: String,
    /// Part row count.
    pub rows: u64,
}

/// Batch digest. It covers the batch number, its positions, and each part
/// reference in sorted order. Admission recomputes it from the batch list.
pub(crate) fn batch_digest(batch_no: u32, first_pos: u64, last_pos: u64, refs: &[BatchRef]) -> String {
    let mut text = String::from(BATCH_PREFIX);
    text.push_str(&format!("{batch_no}|{first_pos}|{last_pos}\n"));
    for part in refs {
        text.push_str(&format!("{}.{}|{}|{}|{}\n", part.schema, part.table, part.part_no, part.sha256, part.rows));
    }
    sha256_hex(text.as_bytes())
}

/// Validated CDC batch list and its per-table position ranges.
pub(crate) struct BatchPlan {
    /// `cdc-batches.json` document.
    pub document: Value,
    /// Chain digest over the ordered batch digests.
    pub chain_sha256: String,
    /// Per table: CDC `part_no` to `(first_pos, last_pos)`.
    pub ranges: Vec<BTreeMap<u32, (u64, u64)>>,
}

/// Validate the batch list and build its document. Batches must number from 1,
/// their positions must chain from `l0_pos + 1` to `l_end_pos`, and each CDC
/// part of each table must appear once, in part order.
pub(crate) fn plan_batches(tables: &[V2Table], batches: &[V2Batch], l0_pos: u64, l_end_pos: u64) -> Result<BatchPlan> {
    let mut expect_part = vec![1u32; tables.len()];
    let mut ranges = vec![BTreeMap::new(); tables.len()];
    let mut previous = l0_pos;
    let mut documented = Vec::with_capacity(batches.len());
    let mut chain = String::from(CHAIN_PREFIX);
    for (index, batch) in batches.iter().enumerate() {
        if u64::from(batch.batch_no) != index as u64 + 1 {
            return v2_fail("batch_number");
        }
        if batch.first_pos != previous + 1 || batch.last_pos < batch.first_pos {
            return v2_fail("batch_chain");
        }
        let mut refs = Vec::with_capacity(batch.refs.len());
        for &(table_index, part_no) in &batch.refs {
            let table = tables.get(table_index).ok_or_else(|| PreviewError::new("baseline_v2", "batch_table"))?;
            if *expect_part.get(table_index).ok_or_else(|| PreviewError::new("baseline_v2", "batch_table"))? != part_no {
                return v2_fail("batch_order");
            }
            expect_part[table_index] += 1;
            let entry = table
                .parts
                .iter()
                .find(|entry| entry.kind == PartKind::Cdc && entry.part_no == part_no)
                .ok_or_else(|| PreviewError::new("baseline_v2", "batch_part"))?;
            ranges[table_index].insert(part_no, (batch.first_pos, batch.last_pos));
            refs.push(BatchRef {
                schema: table.schema.clone(),
                table: table.table.clone(),
                part_no,
                sha256: entry.sha256.clone(),
                rows: entry.rows,
            });
        }
        refs.sort_by(|a, b| (&a.schema, &a.table, a.part_no).cmp(&(&b.schema, &b.table, b.part_no)));
        let digest = batch_digest(batch.batch_no, batch.first_pos, batch.last_pos, &refs);
        chain.push_str(&format!("{}|{}\n", batch.batch_no, digest));
        documented.push(json!({
            "batch_no": batch.batch_no,
            "first_pos": batch.first_pos.to_string(),
            "last_pos": batch.last_pos.to_string(),
            "batch_sha256": digest,
            "parts": refs.iter().map(|part| json!({"schema":part.schema,"table":part.table,"part_no":part.part_no,
                "sha256":part.sha256,"rows":part.rows})).collect::<Vec<_>>(),
        }));
        previous = batch.last_pos;
    }
    for (index, table) in tables.iter().enumerate() {
        let cdc_count = table.parts.iter().filter(|entry| entry.kind == PartKind::Cdc).count() as u32;
        if expect_part[index] != cdc_count + 1 {
            return v2_fail("batch_missing_part");
        }
    }
    if batches.is_empty() {
        if l0_pos != l_end_pos {
            return v2_fail("l_end");
        }
    } else if previous != l_end_pos {
        return v2_fail("l_end");
    }
    let chain_sha256 = sha256_hex(chain.as_bytes());
    let document = json!({
        "protocol": CDC_BATCHES_V2,
        "count": batches.len(),
        "first_pos": batches.first().map(|batch| batch.first_pos.to_string()),
        "last_pos": batches.last().map(|batch| batch.last_pos.to_string()),
        "chain_sha256": chain_sha256,
        "batches": documented,
    });
    Ok(BatchPlan {
        document,
        chain_sha256,
        ranges,
    })
}

/// Position of a GTID set's last transaction: the number after the last `-` or
/// `:`. The source has one UUID per measurement.
pub(crate) fn position_of(gtid: &str) -> Result<u64> {
    gtid.rsplit(['-', ':'])
        .next()
        .and_then(|text| text.parse::<u64>().ok())
        .ok_or_else(|| PreviewError::new("baseline_v2", "gtid_position"))
}

/// Inputs of one v2 materialization.
pub(crate) struct V2Run<'a> {
    /// Admission context from the worker (source id, binary, target, preflight).
    pub context: &'a Value,
    /// Tables in order.
    pub tables: &'a [V2Table],
    /// CDC batches in order.
    pub batches: &'a [V2Batch],
    /// L0 GTID set.
    pub l0: &'a str,
    /// L_end GTID set.
    pub l_end: &'a str,
    /// Lineage (measured UUID).
    pub lineage: &'a str,
    /// Source epoch.
    pub epoch: u32,
    /// Budget and memory policy.
    pub policy: V2Policy,
    /// The producer's admission config, verbatim. Its digest is the `open` frame's `config_digest`.
    pub config: &'a Value,
    /// Type digest and input digest bound at `open` (FRAMES-V2 §3.1, §10).
    pub open_type_digest: &'a str,
    pub open_input_digest: &'a str,
}

/// Admission config v2. The row budget comes from the policy, not a constant.
pub(crate) fn admission_config_v2(context: &Value, tables: &[(String, String)], hard_limit: u64) -> Value {
    json!({"protocol":"oo.source-admission.config/v1","target_id":context["source_id"],"source_id":context["source_id"],
        "secret_ref":context["secret_ref"].as_str().unwrap_or("secret://mysql-native"),"compat_mode":"mysql-native",
        "permissions":tables.iter().map(|(schema,name)| {
            json!({"name":"SELECT","status":"known","granted":true,"scope":{"database":schema,"table":name}}) }).collect::<Vec<_>>(),
        "r1w":null,"budget":{"unit":"Rows","hard_limit":hard_limit,"confidence":"unknown"},
        "write_options":{"signal_table":false,"heartbeat_write":false,"create_stream":false,"create_trigger":false},
        "debezium":{"signal_data_collection":null,"heartbeat_action_query":null,"snapshot_mode":"never","create_stream":false,"create_trigger":false},
        "requested_write_class":"R0"})
}

/// Materialize the v2 baseline. Writes the CDC batch list, the composed expectation,
/// the capturer, and one admission per table into `out`, and returns the final-load
/// request (`oo.pipeline.native-c1-baseline/v2`, FRAMES-V2 §10). That request is the
/// document `import-native-c1-baseline-v2` consumes (T8). The worker adds the lease
/// and fence fields before it calls the loader.
///
/// Written under `out`: `cdc-batches.json`, `capturer.json`, `admission-config.json`,
/// `schema-preimage.json`, `type-preimage.json`, `native-request-v2.json`, one
/// directory per table holding its admission witness, report and seal, and
/// `parts-manifest.json` inside each table's parts directory. The config and the
/// schema digest are the producer's own: they must equal the `open` frame's digests.
pub(crate) fn materialize_v2(out: &Path, run: &V2Run<'_>) -> Result<Value> {
    let context = run.context;
    let source = context["source_id"].as_str().ok_or_else(err)?;
    let generation = context["source_generation"].as_u64().ok_or_else(err)?;
    let admission = context["admission_binary"].as_str().ok_or_else(err)?;
    if sha256_hex(&fs::read(admission).map_err(|_| err())?) != context["admission_sha256"] {
        return Err(err());
    }
    let driver = std::env::current_exe().map_err(|_| err())?;
    let driver_sha = sha256_hex(&fs::read(&driver).map_err(|_| err())?);
    let l0_pos = position_of(run.l0)?;
    let l_end_pos = position_of(run.l_end)?;
    if run.tables.is_empty() || run.policy.max_table_rows == 0 {
        return Err(err());
    }
    let mut names = BTreeSet::new();
    let mut manifests = Vec::with_capacity(run.tables.len());
    let mut types_per_table = Vec::with_capacity(run.tables.len());
    for table in run.tables {
        if !names.insert((table.schema.clone(), table.table.clone())) {
            return Err(err());
        }
        let manifest = PartManifest::new(table.parts.clone()).map_err(lift_parts)?;
        // D7: the canonical types are the source mapping's logical types, one per column.
        let types: Vec<LogicalType> = table.columns.iter().map(|column| column.logical.clone()).collect();
        let keys: Vec<usize> = table.columns.iter().enumerate().filter(|(_, c)| c.key).map(|(i, _)| i).collect();
        let key = match keys.as_slice() {
            [key] => *key,
            _ => return Err(err()),
        };
        if !key_type_ok(types.get(key)) {
            return Err(err());
        }
        types_per_table.push((types, key));
        manifests.push(manifest);
    }
    let plan = plan_batches(run.tables, run.batches, l0_pos, l_end_pos)?;
    let batches_bytes = serde_json::to_vec(&plan.document).map_err(|_| err())?;
    let batches_sha = sha256_hex(&batches_bytes);
    fs::create_dir_all(out).map_err(|_| err())?;
    let batches_path = out.join("cdc-batches.json");
    fs::write(&batches_path, &batches_bytes).map_err(|_| err())?;

    let mut composed = Vec::with_capacity(run.tables.len());
    for (index, table) in run.tables.iter().enumerate() {
        let (types, key) = &types_per_table[index];
        let result = compose_table(&table.parts_dir, &manifests[index], types, *key, &plan.ranges[index], run.policy.max_cdc_keys)?;
        let name = format!("{}.{}", table.schema, table.table);
        let per_table = composed_digest(&[(name.as_str(), &result.digest)]).map_err(lift_parts)?;
        composed.push((name, result, per_table));
    }
    let overall = {
        let pairs: Vec<(&str, &StateDigest)> = composed.iter().map(|(name, result, _)| (name.as_str(), &result.digest)).collect();
        composed_digest(&pairs).map_err(lift_parts)?
    };
    let total_rows: u64 = composed.iter().map(|(_, result, _)| result.rows).sum();

    let identity = json!({"engine":"mysql","measured_id":run.lineage,"shard_id":context["target"]["shard"],"owner_token":context["target"]["owner"]});
    // The producer's `schema-preimage` shape: one `{table, columns}` per table, in the same order.
    let mut schema_docs = Vec::with_capacity(run.tables.len());
    let mut schema_table_bytes = Vec::with_capacity(run.tables.len());
    for table in run.tables {
        let columns: Vec<Value> = table.columns.iter().map(V2Column::schema_json).collect();
        let doc = json!({"table": table.table, "columns": columns});
        schema_table_bytes.push(serde_json::to_vec(&json!([doc.clone()])).map_err(|_| err())?);
        debug_assert_eq!(sha256_hex(schema_table_bytes.last().unwrap()), table_schema_digest(&table.table, &table.columns));
        schema_docs.push(doc);
    }
    let qualified: Vec<String> = run.tables.iter().map(|t| format!("{}.{}", t.schema, t.table)).collect();
    let (type_bytes, type_now) = type_preimage(
        qualified.iter().zip(run.tables.iter()).map(|(name, t)| (name.as_str(), t.table.as_str(), t.columns.as_slice())),
    )?;
    if type_now != run.open_type_digest {
        return Err(err());
    }
    let schema_bytes = serde_json::to_vec(&json!(schema_docs)).map_err(|_| err())?;
    let schema_digest = sha256_hex(&schema_bytes);
    let config = run.config.clone();
    let config_bytes = serde_json::to_vec(&config).map_err(|_| err())?;
    let config_digest = sha256_hex(&config_bytes);
    write(&out.join("admission-config.json"), &config)?;
    fs::write(out.join("schema-preimage.json"), &schema_bytes).map_err(|_| err())?;
    fs::write(out.join("type-preimage.json"), &type_bytes).map_err(|_| err())?;

    let snapshot = json!({"kind":"mysql_cdc_first","value":run.l_end});
    let mut tables_sorted: Vec<usize> = (0..run.tables.len()).collect();
    tables_sorted.sort_by(|a, b| (&run.tables[*a].schema, &run.tables[*a].table).cmp(&(&run.tables[*b].schema, &run.tables[*b].table)));
    let mut scope_tables = Vec::with_capacity(run.tables.len());
    let mut capture_tables = Vec::with_capacity(run.tables.len());
    for &index in &tables_sorted {
        let table = &run.tables[index];
        let (_, result, per_table) = &composed[index];
        let fill_rows: u64 = manifests[index].parts().iter().filter(|e| e.kind == PartKind::Fill).map(|e| e.rows).sum();
        let cdc_rows: u64 = manifests[index].parts().iter().filter(|e| e.kind == PartKind::Cdc).map(|e| e.rows).sum();
        scope_tables.push(json!({"schema":table.schema,"table":table.table,"fill_rows":fill_rows,"cdc_rows":cdc_rows,
            "parts_digest":manifests[index].parts_digest(),"schema_sha256":sha256_hex(&schema_table_bytes[index]),
            "expected_rows":result.rows,"expected_composed_digest":per_table}));
        capture_tables.push(json!({"name":table.table,"table":table.table,"schema":table.schema,"database":table.schema,
            "route":"C1","consistency":"T","delete_mode":"native","cursor_safety":"no_cursor",
            "quality_rejects":[],
            "parts_digest":manifests[index].parts_digest(),"fill_rows":fill_rows,"cdc_rows":cdc_rows}));
    }
    let scope = json!({"protocol":SCOPE_V2,"engine":"mysql","lineage":run.lineage,"l0":run.l0,"l0_pos":l0_pos,
        "l_end":run.l_end,"l_end_pos":l_end_pos,"complete":true,"config_digest":config_digest,"schema_digest":schema_digest,
        "consistency_grade":"T","cross_table_consistent":true,"composition_rule":COMPOSITION_RULE,"bucket_bits":C1_BUCKET_BITS,
        "cdc_batches":{"sha256":batches_sha,"count":run.batches.len(),"chain_sha256":plan.chain_sha256,
            "first_pos":run.batches.first().map(|b| b.first_pos.to_string()),"last_pos":run.batches.last().map(|b| b.last_pos.to_string())},
        "source_identity":identity,"tables":scope_tables,"expected_rows":total_rows,"expected_composed_digest":overall});
    let capturer = json!({"protocol":"oo.mysql.capture-tick/v1","source_id":source,"source_generation":generation,
        "run_id":context["target"]["owner"],"source_preflight":context["source_preflight"],"snapshot":snapshot,
        "tables":capture_tables,"product_reader_write_attempts":context["reader_write_attempts"],"snapshot_token":false,
        "cross_table_consistency":"T","baseline_scope":scope});
    let capturer_path = out.join("capturer.json");
    write(&capturer_path, &capturer)?;
    let capturer_sha = sha256_hex(&fs::read(&capturer_path).map_err(|_| err())?);

    let driver_path = driver.clone();
    let mut tables_out = Vec::with_capacity(run.tables.len());
    for (index, table) in run.tables.iter().enumerate() {
        let (_, result, per_table) = &composed[index];
        let manifest = &manifests[index];
        let manifest_path = table.parts_dir.join("parts-manifest.json");
        let manifest_bytes = serde_json::to_vec(&manifest.to_json()).map_err(|_| err())?;
        fs::create_dir_all(&table.parts_dir).map_err(|_| err())?;
        fs::write(&manifest_path, &manifest_bytes).map_err(|_| err())?;
        let manifest_sha = sha256_hex(&manifest_bytes);
        let dest = out.join(format!("{}.{}", table.schema, table.table));
        fs::create_dir_all(&dest).map_err(|_| err())?;
        fs::write(dest.join("schema-preimage.json"), &schema_table_bytes[index]).map_err(|_| err())?;
        let witness = json!({"protocol":WITNESS_V2,"source_engine":"mysql","source_id":source,"source_generation":generation,
            "snapshot":snapshot,"run_id":context["target"]["owner"],"logical_schema":table.schema,"logical_table":table.table,
            "capturer_path":capturer_path,"table_manifest_path":manifest_path,"table_manifest_sha256":manifest_sha,
            "cdc_batches_path":batches_path,"cdc_batches_sha256":batches_sha,"driver_path":driver_path,
            "schema_preimage_path":dest.join("schema-preimage.json"),"type_preimage_path":out.join("type-preimage.json"),
            "expected_rows":result.rows,"expected_composed_digest":per_table,"composition_rule":COMPOSITION_RULE,"bucket_bits":C1_BUCKET_BITS});
        write(&dest.join("witness.json"), &witness)?;
        let report = json!({"protocol":"oo.source-admission.report/v1","target_id":source,"source_id":source,"secret_ref":config["secret_ref"],
            "compat_mode":"mysql-native","permissions":config["permissions"],"r1w":null,
            "driver":{"kind":"native","name":"mysql_async","version":"0.36.1","digest":driver_sha},"license_flags":["GPL-2.0"],
            "capabilities":[{"name":"Cdc","cursor":null,"feed":null,"cdc":"Binlog","token_order":"Total","budget_unit":"Rows","level":"experimental","write_class":"R0","consistency":"T"}],
            "capture_methods":[{"method":"C1","write_class":"R0","consistency":"T","token_order":"Total","retention_window":format!("{}s",context["source_preflight"]["binlog_expire_logs_seconds"].as_u64().ok_or_else(err)?),"cursor":null,"feed":null,"cdc":"Binlog","delete_detection":"native"}],
            "cost_model":{"unit":"Rows","confidence":"unknown","estimate":"bounded owned fixture"},"capture_witness":dest.join("witness.json")});
        write(&dest.join("report.json"), &report)?;
        let output = Command::new(admission)
            .args(["seal-native", "--report"])
            .arg(dest.join("report.json"))
            .arg("--config")
            .arg(out.join("admission-config.json"))
            .arg("--out")
            .arg(dest.join("sealed"))
            .output()
            .map_err(|_| err())?;
        fs::write(dest.join("seal.stdout"), &output.stdout).map_err(|_| err())?;
        fs::write(dest.join("seal.stderr"), &output.stderr).map_err(|_| err())?;
        fs::write(dest.join("seal.exit"), output.status.code().unwrap_or(-1).to_string()).map_err(|_| err())?;
        if !output.status.success() {
            return Err(err());
        }
        let registry_path = dest.join("sealed/registry.json");
        let registry = fs::read(&registry_path).map_err(|_| err())?;
        let registry_value: Value = serde_json::from_slice(&registry).map_err(|_| err())?;
        let binding = &registry_value["bindings"][0];
        tables_out.push(json!({"schema":table.schema,"table":table.table,
            "run_id":binding["run_id"],
            "type_digest":binding["type_digest"],
            "input_digest":binding["input_digest"],
            "parts_digest":manifest.parts_digest(),
            "admission":{"report_path":dest.join("report.json"),"report_sha256":sha256_hex(&fs::read(dest.join("report.json")).map_err(|_| err())?),
                "witness_path":dest.join("witness.json"),"witness_sha256":sha256_hex(&fs::read(dest.join("witness.json")).map_err(|_| err())?),
                "registry_path":registry_path,"registry_sha256":sha256_hex(&registry),
                "manifest_path":manifest_path,"manifest_sha256":manifest_sha}}));
    }
    // The first registry binds the shared baseline scope, which is the same in every table's registry.
    let first_registry = tables_out.first().ok_or_else(err)?["admission"].clone();
    // Every admission registry binds the same type preimage file, so its digest is the open one.
    if tables_out.iter().any(|t| t["type_digest"].as_str() != Some(type_now.as_str())) {
        return Err(err());
    }
    let type_digest = json!(type_now);
    let input = input_digest(&run.l0, &config_digest, &schema_digest, &type_now);
    if input != run.open_input_digest {
        return Err(err());
    }
    let mut t8_tables = Vec::with_capacity(run.tables.len());
    let mut entries = Vec::with_capacity(run.tables.len());
    for (index, table) in run.tables.iter().enumerate() {
        let (_, result, _) = &composed[index];
        let canonical = table
            .columns
            .iter()
            .map(|column| logical_type_name(&column.logical).map_err(lift_parts))
            .collect::<Result<Vec<_>>>()?;
        t8_tables.push(json!({"schema": table.schema, "table": table.table, "row_count": result.rows,
            "table_digest": hex32(&result.digest.table_digest()), "canonical_types": canonical}));
        entries.push(json!({"schema": table.schema, "table": table.table}));
    }
    let request = json!({"protocol":REQUEST_V2,"source_id":source,"source_generation":generation,
        "source_epoch":run.epoch,"binding":context["binding"],"pin":context["pin"],"target":context["target"],
        "measured_id":run.lineage,"source_identity":identity,
        "l0":run.l0,"l0_pos":l0_pos.to_string(),"l_end":run.l_end,"l_end_pos":l_end_pos.to_string(),
        "consistency_grade":"T","cross_table_consistent":true,
        "baseline_scope":scope,"type_digest":type_digest,"input_digest":input,
        "expected":{"bucket_bits":C1_BUCKET_BITS,"rows":total_rows,"composed_digest":overall,"tables":t8_tables},
        "entries":entries,
        "admission_registry_path":first_registry["registry_path"],"admission_registry_sha256":first_registry["registry_sha256"],
        "cdc_batches":{"path":batches_path,"sha256":batches_sha,"count":run.batches.len(),"chain_sha256":plan.chain_sha256},
        "capturer":{"path":capturer_path,"sha256":capturer_sha},
        "tables":tables_out});
    write(&out.join("native-request-v2.json"), &request)?;
    Ok(request)
}

/// PostEnd entry (DESIGN §4.2, FRAMES-V2 §10). Reads what the producer left in its out
/// directory (`parts-manifest.json`, which carries the columns and the producer's
/// admission config, and `journal.jsonl`, which carries the CDC batches), and runs
/// [`materialize_v2`] on it. `context` is the worker's: source id and generation,
/// the admission binary and its digest, the binding and pin, the target, and the
/// source epoch. Derived output directories from an earlier attempt are removed first.
pub(crate) fn materialize_from_producer(out: &Path, context: &Value) -> Result<Value> {
    let bytes = fs::read(out.join("parts-manifest.json")).map_err(|_| err())?;
    let doc: Value = serde_json::from_slice(&bytes).map_err(|_| err())?;
    if doc["protocol"] != "oo.mysql.c1-parts-manifest/v2" {
        return Err(err());
    }
    let l0 = doc["l0"].as_str().ok_or_else(err)?.to_owned();
    let l_end = doc["l_end"].as_str().ok_or_else(err)?.to_owned();
    let lineage = doc["measured_id"].as_str().ok_or_else(err)?.to_owned();
    let mat = &doc["materialize"];
    // Digests bound at `open` (FRAMES-V2 §3.1): the worker binds these, and the final request must
    // carry exactly them. Materialize checks them against the files it writes.
    let open_type_digest = doc["open_digests"]["type_digest"].as_str().ok_or_else(err)?.to_owned();
    let open_input_digest = doc["open_digests"]["input_digest"].as_str().ok_or_else(err)?.to_owned();
    let mut ctx = context.clone();
    ctx["source_preflight"] = mat["source_preflight"].clone();
    ctx["reader_write_attempts"] = json!(0);
    let mut tables = Vec::new();
    for item in doc["tables"].as_array().ok_or_else(err)? {
        let schema = item["schema"].as_str().ok_or_else(err)?.to_owned();
        let table = item["table"].as_str().ok_or_else(err)?.to_owned();
        let columns = item["columns"]
            .as_array()
            .ok_or_else(err)?
            .iter()
            .map(V2Column::from_facts)
            .collect::<Result<Vec<_>>>()?;
        let parts_dir = out.join("parts").join(format!("{schema}.{table}"));
        let manifest = PartManifest::from_json(&item["manifest"]).map_err(lift_parts)?;
        tables.push(V2Table { schema, table, columns, parts: manifest.parts().to_vec(), parts_dir });
    }
    let mut batches = Vec::new();
    for line in crate::c1_chunked::read_journal(&out.join("journal.jsonl"))? {
        if let JournalLine::Batch(batch) = line {
            let mut refs = Vec::with_capacity(batch.parts.len());
            for part in &batch.parts {
                let index = tables
                    .iter()
                    .position(|t| format!("{}.{}", t.schema, t.table) == part.table)
                    .ok_or_else(err)?;
                refs.push((index, part.part.part_no));
            }
            batches.push(V2Batch {
                batch_no: u32::try_from(batch.batch_no).map_err(|_| err())?,
                first_pos: batch.first_pos,
                last_pos: batch.last_pos,
                refs,
            });
        }
    }
    for dir in tables.iter().map(|t| out.join(format!("{}.{}", t.schema, t.table))) {
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|_| err())?;
        }
    }
    let run = V2Run {
        context: &ctx,
        config: &mat["admission_config"],
        open_type_digest: &open_type_digest,
        open_input_digest: &open_input_digest,
        tables: &tables,
        batches: &batches,
        l0: &l0,
        l_end: &l_end,
        lineage: &lineage,
        epoch: u32::try_from(context["source_epoch"].as_u64().ok_or_else(err)?).map_err(|_| err())?,
        policy: V2Policy {
            max_table_rows: mat["max_table_rows"].as_u64().ok_or_else(err)?,
            max_cdc_keys: usize::try_from(mat["max_cdc_keys"].as_u64().ok_or_else(err)?).map_err(|_| err())?,
        },
    };
    materialize_v2(out, &run)
}

pub(crate) fn temporal_fraction(text: &str) -> Result<(&str, u32)> {
    match text.split_once('.') {
        None => Ok((text, 0)),
        Some((whole, fraction)) if !fraction.is_empty() && fraction.len() <= 6
            && fraction.bytes().all(|b| b.is_ascii_digit()) => {
            let micros = fraction.parse::<u32>().map_err(|_| err())?
                * 10u32.pow(6 - fraction.len() as u32);
            Ok((whole, micros))
        }
        _ => Err(err()),
    }
}

pub(crate) fn raw_cell(cell: &Value, store: StoreKind) -> Result<RawCell> {
    if cell["type"]=="null" {return Ok(RawCell::Null);}
    let text=cell["value"].as_str().ok_or_else(err)?;
    if cell["type"]=="int" {return text.parse::<i64>().map(RawCell::Int).or_else(|_|text.parse::<u64>().map(RawCell::Uint)).map_err(|_|err());}
    if cell["type"]=="bytes" {
        if text.len()%2!=0 {return Err(err());}
        let bytes=text.as_bytes().chunks_exact(2).map(|pair|u8::from_str_radix(std::str::from_utf8(pair).map_err(|_|err())?,16).map_err(|_|err())).collect::<Result<Vec<_>>>()?;
        return Ok(RawCell::Bytes(bytes));
    }
    if matches!(store,StoreKind::Date|StoreKind::TimestampNaive|StoreKind::TimestampTz) {
        let (whole, micros) = temporal_fraction(text.strip_suffix('Z').unwrap_or(text))?;
        let chunks=whole.split(['-','T',' ',':']).map(|s|s.parse::<u32>().map_err(|_|err())).collect::<Result<Vec<_>>>()?;
        if matches!(store,StoreKind::Date) && chunks.len()==3 {
            if whole.len() != text.len() {return Err(err());}
            return Ok(RawCell::Date {year:u16::try_from(chunks[0]).map_err(|_|err())?,month:u8::try_from(chunks[1]).map_err(|_|err())?,day:u8::try_from(chunks[2]).map_err(|_|err())?,hour:0,minute:0,second:0,micros:0});
        }
        if chunks.len()!=6 {return Err(err());}
        return Ok(RawCell::Date {year:u16::try_from(chunks[0]).map_err(|_|err())?,month:u8::try_from(chunks[1]).map_err(|_|err())?,day:u8::try_from(chunks[2]).map_err(|_|err())?,hour:u8::try_from(chunks[3]).map_err(|_|err())?,minute:u8::try_from(chunks[4]).map_err(|_|err())?,second:u8::try_from(chunks[5]).map_err(|_|err())?,micros});
    }
    if matches!(store,StoreKind::Time) {
        let negative=text.starts_with('-');let text=text.strip_prefix('-').unwrap_or(text);
        let (whole, micros) = temporal_fraction(text)?;
        let parts=whole.split(':').map(|s|s.parse::<u32>().map_err(|_|err())).collect::<Result<Vec<_>>>()?;
        if parts.len()!=3 {return Err(err());}
        return Ok(RawCell::Time {negative,days:parts[0]/24,hours:u8::try_from(parts[0]%24).map_err(|_|err())?,minutes:u8::try_from(parts[1]).map_err(|_|err())?,seconds:u8::try_from(parts[2]).map_err(|_|err())?,micros});
    }
    Ok(RawCell::Bytes(text.as_bytes().to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn temporal_fraction_uses_decimal_seconds_not_integer_microseconds() {
        for (fraction, expected) in [("1", 100000), ("123", 123000), ("000001", 1), ("999999", 999999)] {
            let raw = raw_cell(&json!({"type":"text","value":format!("1969-12-31T23:59:59.{fraction}Z")}),
                StoreKind::TimestampTz).unwrap();
            assert!(matches!(raw, RawCell::Date {micros, ..} if micros == expected));
            let raw = raw_cell(&json!({"type":"text","value":format!("-25:00:00.{fraction}")}),
                StoreKind::Time).unwrap();
            assert!(matches!(raw, RawCell::Time {negative:true,days:1,hours:1,micros, ..} if micros == expected));
        }
        for text in ["2024-01-01T00:00:00.", "2024-01-01T00:00:00.0000001",
                     "2024-01-01T00:00:00.1.2", "2024-01-01T00:00:00.1ZZ"] {
            assert!(raw_cell(&json!({"type":"text","value":text}), StoreKind::TimestampNaive).is_err());
        }
        assert!(raw_cell(&json!({"type":"text","value":"--25:00:00"}), StoreKind::Time).is_err());
        assert!(raw_cell(&json!({"type":"text","value":"2024-01-01.1"}), StoreKind::Date).is_err());
    }

    #[test]
    fn stage_cells_keep_large_unsigned_binary_null_and_signed_time() {
        assert!(matches!(raw_cell(&json!({"type":"int","value":"18446744073709551615"}),StoreKind::Numeric{precision:20,scale:0}).unwrap(),RawCell::Uint(u64::MAX)));
        assert!(matches!(raw_cell(&json!({"type":"bytes","value":"00ff80"}),StoreKind::Bytes).unwrap(),RawCell::Bytes(v) if v==vec![0,255,128]));
        assert!(matches!(raw_cell(&json!({"type":"null"}),StoreKind::Bytes).unwrap(),RawCell::Null));
        assert!(matches!(raw_cell(&json!({"type":"text","value":"-25:00:00.000001"}),StoreKind::Time).unwrap(),RawCell::Time{negative:true,days:1,hours:1,micros:1,..}));
        assert!(raw_cell(&json!({"type":"bytes","value":"0z"}),StoreKind::Bytes).is_err());
        assert!(raw_cell(&json!({"type":"bytes","value":"aåb"}),StoreKind::Bytes).is_err());
    }
    #[test]
    fn date_only_and_zero_date_follow_existing_nullable_quality_policy() {
        let raw=raw_cell(&json!({"type":"text","value":"2024-01-02"}),StoreKind::Date).unwrap();
        assert!(values::decode_cell(StoreKind::Date,&raw,true).unwrap().1.is_none());
        let raw=raw_cell(&json!({"type":"text","value":"0000-00-00"}),StoreKind::Date).unwrap();
        assert_eq!(values::decode_cell(StoreKind::Date,&raw,true).unwrap().1.unwrap().reason,"zero_date");
        assert!(values::decode_cell(StoreKind::Date,&raw,false).is_err());
    }

}

#[cfg(test)]
mod v2_tests {
    use super::*;
    use crate::c1_model::{CdcOp, GenParams, ModelOptions, ReadPolicy, Rng, Scenario, TableShape, table_digest};
    use oo_pipeline::c1_parts::{part_entry_for_file, part_file_name};
    use oo_pipeline::native_arrow::{DecodedField, DecodedStream};

    fn fact<T>(result: Result<T>) -> &'static str {
        match result {
            Ok(_) => "ok",
            Err(err) => err.fact,
        }
    }

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/c1v2")
    }

    /// A fresh directory holding the committed fixture parts under their manifest
    /// names (`fill-00000001.arrow` and so on). The fixtures are stored as `.ipc`
    /// because the repository ignores `*.arrow`.
    fn staged_parts() -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "oo-c1-v2-parts-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        for name in ["fill-00000001", "fill-00000002", "cdc-00000001", "cdc-00000002"] {
            fs::copy(fixtures().join(format!("{name}.ipc")), dir.join(format!("{name}.arrow"))).unwrap();
        }
        dir
    }

    fn int_text() -> Vec<LogicalType> {
        vec![LogicalType::Int, LogicalType::Text]
    }

    fn row(key: i128, name: &str) -> Vec<Cell> {
        vec![Cell::Int(key), Cell::Text(name.to_owned())]
    }

    /// Composition of a generated history, fed through the composer exactly as the
    /// streaming path feeds it: CDC rows, fill rows, CDC rows again.
    fn compose_scenario(scenario: &Scenario) -> (u64, String) {
        let schema = scenario.history.schema();
        let mut composers: Vec<Composer> = schema
            .iter()
            .map(|shape| Composer::new(shape.types().to_vec(), shape.key(), usize::MAX).unwrap())
            .collect();
        for (table, composer) in composers.iter_mut().enumerate() {
            for batch in &scenario.batches {
                for event in batch.events.iter().filter(|e| e.table == table) {
                    composer
                        .note_cdc(event.key, event.pos, matches!(event.op, CdcOp::Upsert(_)))
                        .unwrap();
                }
            }
        }
        for (table, composer) in composers.iter_mut().enumerate() {
            composer.begin_fills().unwrap();
            for chunk in &scenario.chunks[table] {
                for row in &chunk.rows {
                    composer.add_fill(row).unwrap();
                }
            }
        }
        for (table, composer) in composers.iter_mut().enumerate() {
            composer.begin_images().unwrap();
            for batch in &scenario.batches {
                for event in batch.events.iter().filter(|e| e.table == table) {
                    let image = match &event.op {
                        CdcOp::Upsert(row) => Some(row.as_slice()),
                        CdcOp::Delete => None,
                    };
                    composer.add_image(event.key, image).unwrap();
                }
            }
        }
        let mut rows = 0;
        let mut named = Vec::new();
        for (shape, composer) in schema.iter().zip(composers) {
            let (count, digest) = composer.finish().unwrap();
            rows += count;
            named.push((shape.name().to_owned(), digest));
        }
        let refs: Vec<(&str, &StateDigest)> = named.iter().map(|(n, d)| (n.as_str(), d)).collect();
        (rows, composed_digest(&refs).unwrap())
    }

    #[test]
    fn composer_matches_the_reference_model_on_random_histories() {
        for (params, seeds) in [(GenParams::standard(), 0..300u64), (GenParams::exhaustive(), 0..200u64)] {
            for seed in seeds {
                let scenario = Scenario::generate(seed, &params, ReadPolicy::AfterL0).unwrap();
                let steps = scenario.random_steps(&mut Rng::new(seed ^ 0xA5A5));
                let outcome = scenario.run(&steps, ModelOptions::new(C1_BUCKET_BITS)).unwrap();
                assert!(outcome.matches(), "model diverged from truth, seed {seed}");
                let (rows, digest) = compose_scenario(&scenario);
                assert_eq!(rows, outcome.model.rows, "rows, seed {seed}");
                assert_eq!(digest, outcome.model.composed_digest, "digest, seed {seed}");
                assert_eq!(digest, outcome.truth.composed_digest, "truth, seed {seed}");
            }
        }
    }

    #[test]
    fn touched_keys_take_their_final_image_or_disappear() {
        // Fill keys 1,2,3. CDC: 2 upserts at 5, 3 deletes at 6, 7 upserts at 4.
        // Expected: 1 (fill), 2 (CDC image), 7 (CDC). Key 3 is gone.
        let types = int_text();
        let mut composer = Composer::new(types.clone(), 0, 8).unwrap();
        composer.note_cdc(2, 5, true).unwrap();
        composer.note_cdc(3, 6, false).unwrap();
        composer.note_cdc(7, 4, true).unwrap();
        composer.begin_fills().unwrap();
        for key in 1..=3 {
            composer.add_fill(&row(key, "fill")).unwrap();
        }
        composer.begin_images().unwrap();
        composer.add_image(2, Some(&row(2, "cdc")[..])).unwrap();
        composer.add_image(3, None).unwrap();
        composer.add_image(7, Some(&row(7, "g")[..])).unwrap();
        let (rows, digest) = composer.finish().unwrap();
        assert_eq!(rows, 3);
        let shape = TableShape::new("t", types, 0).unwrap();
        let expected = vec![row(1, "fill"), row(2, "cdc"), row(7, "g")];
        assert_eq!(digest, table_digest(C1_BUCKET_BITS, &shape, &expected).unwrap());
    }

    #[test]
    fn newest_position_wins_regardless_of_arrival_order() {
        let types = int_text();
        let mut composer = Composer::new(types.clone(), 0, 8).unwrap();
        // Key 4: delete at 9 arrives before an older upsert at 2.
        composer.note_cdc(4, 9, false).unwrap();
        composer.note_cdc(4, 2, true).unwrap();
        composer.begin_fills().unwrap();
        composer.begin_images().unwrap();
        composer.add_image(4, None).unwrap();
        composer.add_image(4, Some(&row(4, "old")[..])).unwrap();
        let (rows, _) = composer.finish().unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn composer_refuses_ties_disorder_bounds_and_pass_mismatch() {
        let types = int_text();
        let mut c = Composer::new(types.clone(), 0, 8).unwrap();
        c.note_cdc(5, 3, true).unwrap();
        assert_eq!(fact(c.note_cdc(5, 3, false)), "cdc_tie");
        assert_eq!(fact(c.note_cdc(5, 0, false)), "cdc_pos");

        let mut bounded = Composer::new(types.clone(), 0, 1).unwrap();
        bounded.note_cdc(1, 1, true).unwrap();
        assert_eq!(fact(bounded.note_cdc(2, 2, true)), "cdc_keys_exceed");

        let mut fills = Composer::new(types.clone(), 0, 8).unwrap();
        fills.begin_fills().unwrap();
        fills.add_fill(&row(9, "x")).unwrap();
        assert_eq!(fact(fills.add_fill(&row(9, "again"))), "fill_order");

        let mut unfinished = Composer::new(types.clone(), 0, 8).unwrap();
        unfinished.note_cdc(1, 1, true).unwrap();
        unfinished.begin_fills().unwrap();
        unfinished.begin_images().unwrap();
        assert_eq!(fact(unfinished.finish()), "cdc_pass");

        let mut upsert_without_image = Composer::new(types.clone(), 0, 8).unwrap();
        upsert_without_image.note_cdc(1, 1, true).unwrap();
        upsert_without_image.begin_fills().unwrap();
        upsert_without_image.begin_images().unwrap();
        assert_eq!(fact(upsert_without_image.add_image(1, None)), "cdc_image");

        assert_eq!(fact(Composer::new(vec![LogicalType::Text], 0, 8)), "key_type");
    }

    #[test]
    fn cell_mapping_follows_the_target_logical_type() {
        assert_eq!(cell_of(&LogicalType::Int, &NativeCell::Int { bit_width: 64, value: -7 }).unwrap(), Cell::Int(-7));
        assert_eq!(cell_of(&LogicalType::Int, &NativeCell::UInt64(u64::MAX)).unwrap(), Cell::Int(i128::from(u64::MAX)));
        assert_eq!(
            cell_of(&LogicalType::Decimal { precision: 10, scale: 2 }, &NativeCell::Decimal {
                precision: 10, scale: 2, coefficient: 1250 }).unwrap(),
            Cell::Decimal("12.50".into())
        );
        assert_eq!(
            cell_of(&LogicalType::Date, &NativeCell::Date32(0)).unwrap(),
            Cell::Date { year: 1970, month: 1, day: 1 }
        );
        assert_eq!(
            cell_of(&LogicalType::TimestampTz, &NativeCell::TimestampNanos { value: 1_000_000, timezone: "UTC".into() }).unwrap(),
            Cell::Micros(1000)
        );
        assert_eq!(
            cell_of(&LogicalType::Text, &NativeCell::Utf8("ünï".into())).unwrap(),
            Cell::Text("ünï".into())
        );
        assert_eq!(cell_of(&LogicalType::Bool, &NativeCell::Null).unwrap(), Cell::Null);
        assert_eq!(fact(cell_of(&LogicalType::Int, &NativeCell::Utf8("1".into()))), "cell_type");
        assert_eq!(
            fact(cell_of(&LogicalType::TimestampNaive, &NativeCell::TimestampNanos { value: 1_500, timezone: "UTC".into() })),
            "cell_type"
        );
    }

    /// F3. A `BIGINT UNSIGNED` key is a scale-0 decimal. Its maximum composes like any key,
    /// and a non-integer decimal key is still refused.
    #[test]
    fn unsigned_bigint_keys_compose_up_to_u64_max() {
        let types = vec![LogicalType::Decimal { precision: 20, scale: 0 }, LogicalType::Text];
        let max = i128::from(u64::MAX);
        let mut composer = Composer::new(types, 0, 8).expect("decimal key");
        composer.note_cdc(max, 5, true).unwrap();
        composer.begin_fills().unwrap();
        let row = vec![Cell::Decimal("18446744073709551615".into()), Cell::Text("m".into())];
        assert_eq!(composer.add_fill(&row).unwrap(), max);
        composer.begin_images().unwrap();
        composer.add_image(max, Some(row.as_slice())).unwrap();
        let (rows, _) = composer.finish().unwrap();
        assert_eq!(rows, 1);
        assert_eq!(
            fact(Composer::new(vec![LogicalType::Decimal { precision: 10, scale: 2 }, LogicalType::Text], 0, 8).map(|_| ())),
            "key_type"
        );
        assert!(key_type_ok(Some(&LogicalType::Int)) && key_type_ok(Some(&LogicalType::Decimal { precision: 20, scale: 0 })));
        assert!(!key_type_ok(Some(&LogicalType::Text)));
    }

    #[test]
    fn decimal_text_and_calendar_are_exact() {
        assert_eq!(decimal_text(-5, 2).unwrap(), "-0.05");
        assert_eq!(decimal_text(0, 0).unwrap(), "0");
        assert_eq!(decimal_text(123, 0).unwrap(), "123");
        assert_eq!(decimal_text(7, 3).unwrap(), "0.007");
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(19_000), (2022, 1, 8));
    }

    /// Entries of the committed fixture table `s.t`: two fill chunks and two CDC
    /// batches. Batch 1 covers positions 11..=15 and batch 2 covers 16..=18.
    fn fixture_parts(dir: &Path) -> Vec<PartEntry> {
        let fill = |no: u32, lo: i128, hi: i128, rows: u64| {
            part_entry_for_file(&dir.join(part_file_name(PartKind::Fill, no).unwrap()), no, PartKind::Fill, Some(lo), Some(hi), rows).unwrap()
        };
        let cdc = |no: u32, lo: i128, hi: i128, rows: u64| {
            part_entry_for_file(&dir.join(part_file_name(PartKind::Cdc, no).unwrap()), no, PartKind::Cdc, Some(lo), Some(hi), rows).unwrap()
        };
        vec![fill(1, 1, 3, 3), fill(2, 10, 11, 2), cdc(1, 2, 7, 3), cdc(2, 2, 11, 2)]
    }

    fn fixture_batches() -> Vec<V2Batch> {
        vec![
            V2Batch { batch_no: 1, first_pos: 11, last_pos: 15, refs: vec![(0, 1)] },
            V2Batch { batch_no: 2, first_pos: 16, last_pos: 18, refs: vec![(0, 2)] },
        ]
    }

    fn fixture_table_value() -> V2Table {
        let dir = staged_parts();
        V2Table {
            schema: "s".into(),
            table: "t".into(),
            columns: Vec::new(),
            parts: fixture_parts(&dir),
            parts_dir: dir,
        }
    }

    #[test]
    fn arrow_parts_compose_to_the_expected_state() {
        let table = fixture_table_value();
        let manifest = PartManifest::new(table.parts.clone()).unwrap();
        let plan = plan_batches(std::slice::from_ref(&table), &fixture_batches(), 10, 18).unwrap();
        let types = int_text();
        let composed = compose_table(&table.parts_dir, &manifest, &types, 0, &plan.ranges[0], usize::MAX).unwrap();
        // Expected: 1 a (fill), 2 B2b (CDC image at 18), 7 g (CDC), 10 x (fill).
        let shape = TableShape::new("t", types, 0).unwrap();
        let expected = vec![row(1, "a"), row(2, "B2b"), row(7, "g"), row(10, "x")];
        assert_eq!(composed.rows, 4);
        assert_eq!(composed.digest, table_digest(C1_BUCKET_BITS, &shape, &expected).unwrap());
    }

    #[test]
    fn single_part_table_keeps_the_file_digest() {
        let dir = staged_parts();
        let only = part_entry_for_file(&dir.join("fill-00000002.arrow"), 1, PartKind::Fill, Some(10), Some(11), 2).unwrap();
        let manifest = PartManifest::new(vec![only.clone()]).unwrap();
        assert_eq!(manifest.parts_digest(), only.sha256);
    }

    #[test]
    fn tampered_or_missing_parts_are_refused_while_composing() {
        let scratch = staged_parts();
        let parts = fixture_parts(&scratch);
        let manifest = PartManifest::new(parts.clone()).unwrap();
        let types = int_text();
        let plan = plan_batches(&[V2Table { parts: parts.clone(), ..fixture_table_value() }], &fixture_batches(), 10, 18).unwrap();
        // A flipped byte fails the manifest SHA before any row is composed.
        let original = fs::read(scratch.join("cdc-00000002.arrow")).unwrap();
        let mut bytes = original.clone();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        fs::write(scratch.join("cdc-00000002.arrow"), &bytes).unwrap();
        assert_eq!(fact(compose_table(&scratch, &manifest, &types, 0, &plan.ranges[0], usize::MAX)), "part_sha256");
        // A missing fill part fails to open. CDC parts are read first, and they are intact here.
        fs::write(scratch.join("cdc-00000002.arrow"), &original).unwrap();
        fs::remove_file(scratch.join("fill-00000002.arrow")).unwrap();
        assert_eq!(fact(compose_table(&scratch, &manifest, &types, 0, &plan.ranges[0], usize::MAX)), "part_open");
        fs::remove_dir_all(&scratch).unwrap();
    }

    #[test]
    fn batch_list_is_validated_before_any_part_is_read() {
        let table = fixture_table_value();
        let tables = std::slice::from_ref(&table);
        let good = fixture_batches();
        assert!(plan_batches(tables, &good, 10, 18).is_ok());
        // Reordered batches: the chain from position 10 breaks.
        let reordered = vec![
            V2Batch { batch_no: 1, first_pos: 16, last_pos: 18, refs: vec![(0, 2)] },
            V2Batch { batch_no: 2, first_pos: 11, last_pos: 15, refs: vec![(0, 1)] },
        ];
        assert_eq!(fact(plan_batches(tables, &reordered, 10, 18)), "batch_chain");
        // A CDC part that no batch carries.
        let partial = vec![V2Batch { batch_no: 1, first_pos: 11, last_pos: 15, refs: vec![(0, 1)] }];
        assert_eq!(fact(plan_batches(tables, &partial, 10, 15)), "batch_missing_part");
        // A reference to a part number out of order.
        let skipped = vec![V2Batch { batch_no: 1, first_pos: 11, last_pos: 18, refs: vec![(0, 2), (0, 1)] }];
        assert_eq!(fact(plan_batches(tables, &skipped, 10, 18)), "batch_order");
        // The batches must end at L_end.
        assert_eq!(fact(plan_batches(tables, &good, 10, 19)), "l_end");
    }

    /// Golden digests shared with source-admission (`c1_v2_tests`). Admission
    /// computes the same values with its own code, so a drift in either side fails.
    const GOLDEN_PARTS_DIGEST: &str = "d0826b6ea78b56aed4ef6878c65f4a16077baa552a0d5a3316ae549d555f27e3";
    const GOLDEN_BATCH_DIGEST: &str = "2afcb6226ae87da9bb44f420844662f2251917e95c0aef4f31cce2e30b86d1df";

    #[test]
    fn golden_digests_are_shared_with_admission() {
        let manifest = PartManifest::new(fixture_parts(&staged_parts())).unwrap();
        assert_eq!(manifest.parts_digest(), GOLDEN_PARTS_DIGEST);
        let refs = vec![BatchRef { schema: "s".into(), table: "t".into(), part_no: 1,
            sha256: "ab".repeat(32), rows: 3 }];
        assert_eq!(batch_digest(1, 11, 15, &refs), GOLDEN_BATCH_DIGEST);
    }

    #[test]
    fn position_is_the_last_gtid_number() {
        assert_eq!(position_of("9f2c-1-5230").unwrap(), 5230);
        assert_eq!(position_of("9f2c:1-100").unwrap(), 100);
        assert_eq!(position_of("9f2c:7").unwrap(), 7);
        assert_eq!(fact(position_of("no-number-here")), "gtid_position");
    }

    #[test]
    fn input_digest_golden_vector_matches_the_admission() {
        // Golden value shared with source-admission `input_digest_v2` (same formula, same vector).
        assert_eq!(
            input_digest("u:1-5", &"a".repeat(64), &"b".repeat(64), &"c".repeat(64)),
            "b01997b5e1399dc2a04ffe16f303d3230f48c8696599c539753803a23884a1eb"
        );
    }

    #[test]
    fn admission_config_budget_is_the_policy_value() {
        let context = json!({"source_id":"s1","secret_ref":"secret://x"});
        let config = admission_config_v2(&context, &[("s".into(), "t".into())], 50_000_000);
        assert_eq!(config["budget"]["hard_limit"], json!(50_000_000u64));
        assert_eq!(config["budget"]["unit"], json!("Rows"));
        assert_eq!(V2Policy::default().max_table_rows, 50_000_000);
    }

    /// End to end: `materialize_v2` writes the manifests, the batch list and the
    /// capturer, then runs the real `seal-native` for the table. Run with
    /// `OO_C1_V2_ADMISSION_BIN=<oo-source-admission release binary>
    /// cargo test -- --ignored materialize_v2_end_to_end`.
    #[test]
    #[ignore]
    fn materialize_v2_end_to_end_with_the_admission_binary() {
        let admission = PathBuf::from(std::env::var_os("OO_C1_V2_ADMISSION_BIN").expect("OO_C1_V2_ADMISSION_BIN"));
        let parts_dir = staged_parts();
        let scratch = parts_dir.clone();
        let out = scratch.join("out");
        let owner = "0d9f7c2e-1b3a-4c5d-8e9f-a0b1c2d3e4f5";
        let uuid = "3f1a2b3c-4d5e-4f60-8172-93a4b5c6d7e8";
        let context = json!({"source_id": "s1", "source_generation": 1,
            "admission_binary": admission, "admission_sha256": sha256_hex(&fs::read(&admission).unwrap()),
            "secret_ref": "secret://mysql-native", "binding": "bind-1", "pin": 1,
            "target": {"database": "oo_target", "owner": owner, "shard": "shard-a"},
            "source_preflight": {"gtid_mode": "ON", "enforce_gtid_consistency": "ON", "binlog_format": "ROW",
                "binlog_row_image": "FULL", "log_bin": "1", "binlog_expire_logs_seconds": 604800},
            "reader_write_attempts": 0});
        let columns = vec![
            V2Column { name: "id".into(), attnum: 1, source_type: "bigint".into(), nullable: false, key: true,
                oid: 20, typmod: -1, caveat: None, arrow: ("int64".into(), None, None), logical: LogicalType::Int },
            V2Column { name: "name".into(), attnum: 2, source_type: "varchar".into(), nullable: true, key: false,
                oid: 1043, typmod: -1, caveat: None, arrow: ("utf8".into(), None, None), logical: LogicalType::Text },
        ];
        let tables = vec![V2Table { schema: "s".into(), table: "t".into(), columns, parts_dir: parts_dir.clone(),
            parts: fixture_parts(&parts_dir) }];
        let batches = fixture_batches();
        let config = admission_config_v2(&context, &[("s".into(), "t".into())], V2Policy::default().max_table_rows);
        let l0 = format!("{uuid}:1-10");
        // What `open` computes from the same facts: the type digest and the input digest.
        let qualified: Vec<String> = tables.iter().map(|t| format!("{}.{}", t.schema, t.table)).collect();
        let (_, open_type) = type_preimage(
            qualified.iter().zip(tables.iter()).map(|(q, t)| (q.as_str(), t.table.as_str(), t.columns.as_slice())),
        )
        .expect("type");
        let schema_docs: Vec<Value> = tables
            .iter()
            .map(|t| json!({"table": t.table, "columns": t.columns.iter().map(V2Column::schema_json).collect::<Vec<_>>()}))
            .collect();
        let schema_digest = sha256_hex(&serde_json::to_vec(&json!(schema_docs)).unwrap());
        let open_input = input_digest(&l0, &sha256_hex(&serde_json::to_vec(&config).unwrap()), &schema_digest, &open_type);
        let run = V2Run { context: &context, config: &config, tables: &tables, batches: &batches,
            l0: &l0, l_end: &format!("{uuid}:1-18"), lineage: &format!("mysql.{uuid}"),
            epoch: 1, policy: V2Policy::default(), open_type_digest: &open_type, open_input_digest: &open_input };
        let request = materialize_v2(&out, &run).expect("materialize_v2");
        // Single source of truth (FRAMES-V2 §3.1): the request and the admission carry the digests
        // that `open` bound, not values recomputed at the end.
        assert_eq!(request["type_digest"], json!(open_type));
        assert_eq!(request["input_digest"], json!(open_input));
        assert_eq!(request["expected"]["tables"][0]["table"], json!(tables[0].table));
        // The final-load request (T8) shape: one expectation per table, one registry, no positions.
        assert_eq!(request["protocol"], json!(REQUEST_V2));
        assert_eq!(request["expected"]["rows"], json!(4));
        assert_eq!(request["expected"]["bucket_bits"], json!(C1_BUCKET_BITS));
        assert_eq!(request["expected"]["tables"][0]["canonical_types"], json!(["int", "text"]));
        assert!(request.get("row_positions").is_none() && request["tables"][0].get("read_fence_digest").is_none());
        let shape = TableShape::new("t", int_text(), 0).unwrap();
        let expected = vec![row(1, "a"), row(2, "B2b"), row(7, "g"), row(10, "x")];
        let digest = table_digest(C1_BUCKET_BITS, &shape, &expected).unwrap();
        let composed = composed_digest(&[("s.t", &digest)]).unwrap();
        assert_eq!(request["expected"]["composed_digest"], json!(composed));
        assert_eq!(request["expected"]["tables"][0]["table_digest"], json!(hex32(&digest.table_digest())));
        let registry: Value = serde_json::from_slice(&fs::read(out.join("s.t/sealed/registry.json")).unwrap()).unwrap();
        assert_eq!(registry["bindings"][0]["c1_parts"]["parts_digest"], request["tables"][0]["parts_digest"]);
        assert_eq!(request["admission_registry_sha256"], request["tables"][0]["admission"]["registry_sha256"]);
        println!("e2e request: {}", serde_json::to_string_pretty(&request).unwrap());
        fs::remove_dir_all(&scratch).ok();
    }

    /// `VmHWM` of this process in kB (Linux). `None` where `/proc` is absent.
    fn peak_rss_kb() -> Option<u64> {
        let status = fs::read_to_string("/proc/self/status").ok()?;
        status
            .lines()
            .find(|line| line.starts_with("VmHWM:"))
            .and_then(|line| line.split_whitespace().nth(1)?.parse().ok())
    }

    /// Peak memory of the streaming composition over fill parts of about 200 MiB.
    /// Run with `OO_C1_V2_RSS_DIR=<dir of fill-*.arrow> OO_C1_V2_RSS_ROWS=<rows per
    /// part> cargo test --release -- --ignored rss_`. Keys are contiguous, one
    /// chunk per part, and the table has the (Int, Text) shape.
    #[test]
    #[ignore]
    fn rss_streaming_composition_over_large_parts() {
        let dir = PathBuf::from(std::env::var_os("OO_C1_V2_RSS_DIR").expect("OO_C1_V2_RSS_DIR"));
        let rows: i128 = std::env::var("OO_C1_V2_RSS_ROWS").ok().and_then(|v| v.parse().ok()).unwrap_or(30_000);
        let mut names: Vec<PathBuf> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
        names.sort();
        let mut entries = Vec::new();
        let mut bytes_total = 0u64;
        for (index, path) in names.iter().enumerate() {
            let lo = index as i128 * rows + 1;
            let entry = part_entry_for_file(path, index as u32 + 1, PartKind::Fill, Some(lo), Some(lo + rows - 1), rows as u64).unwrap();
            bytes_total += entry.bytes;
            entries.push(entry);
        }
        let manifest = PartManifest::new(entries).unwrap();
        // Reset the high-water mark so it covers only the composition below.
        let _ = fs::write("/proc/self/clear_refs", "5");
        let before = peak_rss_kb();
        let started = std::time::Instant::now();
        let composed = compose_table(&dir, &manifest, &int_text(), 0, &BTreeMap::new(), usize::MAX).unwrap();
        let elapsed = started.elapsed();
        let after = peak_rss_kb();
        println!(
            "rss-composition parts={} bytes={} rows={} hwm_reset_kb={:?} hwm_peak_during_kb={:?} seconds={:.2}",
            manifest.parts().len(), bytes_total, composed.rows, before, after, elapsed.as_secs_f64()
        );
        assert_eq!(composed.rows, rows as u64 * manifest.parts().len() as u64);
    }

    #[test]
    fn decoded_stream_rows_read_by_column_index() {
        let stream = DecodedStream {
            rows: 1,
            fields: vec![DecodedField {
                name: "id".into(),
                nullable: false,
                arrow_type: "int64".into(),
                precision: None,
                scale: None,
                timezone: None,
            }],
            columns: vec![vec![NativeCell::Int { bit_width: 64, value: 4 }]],
        };
        assert_eq!(data_row(&stream, 0, &[LogicalType::Int]).unwrap(), vec![Cell::Int(4)]);
        assert_eq!(fact(data_row(&stream, 1, &[LogicalType::Int])), "part_row");
    }
}
