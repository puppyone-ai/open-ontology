//! PG-only verifier shared by admission and the existing native loader.
//! Reads original immutable captures, never a database or a caller trust bit.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};
pub type Result<T> = std::result::Result<T, &'static str>;
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn text<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str().filter(|s| !s.is_empty()).ok_or("pg_field")
}
pub fn lsn(s: &str) -> Result<u64> {
    let (a, b) = s.split_once('/').ok_or("pg_lsn")?;
    Ok(
        (u64::from(u32::from_str_radix(a, 16).map_err(|_| "pg_lsn")?) << 32)
            | u64::from(u32::from_str_radix(b, 16).map_err(|_| "pg_lsn")?),
    )
}
pub fn ident(s: &str) -> bool {
    !s.is_empty() && s.len() <= 63 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub path: PathBuf,
    pub sha256: String,
}
impl Artifact {
    pub fn bytes(&self) -> Result<Vec<u8>> {
        if !self.path.is_absolute()
            || self
                .path
                .components()
                .any(|p| p == std::path::Component::ParentDir)
        {
            return Err("pg_path");
        }
        let b = std::fs::read(&self.path).map_err(|_| "pg_file")?;
        if hash(&b) != self.sha256 {
            return Err("pg_digest");
        }
        Ok(b)
    }
    pub fn document(&self) -> Result<Value> {
        serde_json::from_slice(&self.bytes()?).map_err(|_| "pg_json")
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub protocol: String,
    pub source_id: String,
    pub baseline: Artifact,
    pub transactions: Artifact,
    pub baseline_config: Artifact,
    pub transactions_config: Artifact,
    pub baseline_driver: Artifact,
    pub transactions_driver: Artifact,
}
pub struct Verified {
    pub bundle: Bundle,
    pub baseline: Value,
    pub transactions: Value,
    pub source_config: Value,
    pub digest: String,
}
const DRIVERS: &[&str] = &[
    "07b6f3b75acbbda4280bec9c0971d081402eecbc8e721d2e0df3ef651416e4a0",
    "17dfb172a493833ccccaecd5ee0036474bde0a3601b755de4b3d13f307e02460",
];
pub fn part(root: &Path, v: &Value) -> Result<Artifact> {
    let name = text(v, "path")?;
    if Path::new(name).components().count() != 1 {
        return Err("pg_part_path");
    }
    let path = root.join(name);
    if path.canonicalize().map_err(|_| "pg_part_path")?.parent()
        != Some(root.canonicalize().map_err(|_| "pg_part_path")?.as_path())
    {
        return Err("pg_part_path");
    }
    let artifact = Artifact {
        path,
        sha256: text(v, "sha256")?.into(),
    };
    let meta = std::fs::metadata(&artifact.path).map_err(|_| "pg_part")?;
    if Some(meta.len()) != v["bytes"].as_u64() || meta.len() > 64 * 1024 * 1024 {
        return Err("pg_part_size");
    }
    Ok(artifact)
}
pub fn verify(pin: &Artifact) -> Result<Verified> {
    let bundle: Bundle = serde_json::from_slice(&pin.bytes()?).map_err(|_| "pg_bundle")?;
    if bundle.protocol != "oo.postgres-preview.target-bundle/v1" || !ident(&bundle.source_id) {
        return Err("pg_bundle");
    }
    let baseline = bundle.baseline.document()?;
    let transactions = bundle.transactions.document()?;
    let configs = [
        bundle.baseline_config.document()?,
        bundle.transactions_config.document()?,
    ];
    for (m, config, cp, d) in [
        (
            &baseline,
            &configs[0],
            &bundle.baseline_config,
            &bundle.baseline_driver,
        ),
        (
            &transactions,
            &configs[1],
            &bundle.transactions_config,
            &bundle.transactions_driver,
        ),
    ] {
        if m["protocol"] != "oo.postgres-preview.capture-set/v1"
            || m["complete"] != true
            || m["source_id"] != bundle.source_id
            || m["source_read_only"] != true
            || m["source_dml"] != 0
            || m["slot_advanced"] != false
            || m["worker_ack"] != false
            || m["target_applied"] != false
            || m["admission_decision"] != "not_issued"
            || m["driver_sha256"] != d.sha256
            || !DRIVERS.contains(&d.sha256.as_str())
            || m["config_sha256"] != cp.sha256
        {
            return Err("pg_capture_identity");
        }
        d.bytes()?;
        if config["protocol"] != "oo.postgres-preview.capture-config/v1"
            || config["source_id"] != bundle.source_id
            || config["source"]["host"] != m["source_identity"]["host"]
            || config["source"]["port"] != m["source_identity"]["port"]
            || config["source"]["database"] != m["source_identity"]["database"]
        {
            return Err("pg_config_identity");
        }
        for k in ["system_identifier", "database_oid", "slot", "publication"] {
            if config["capture"][k] != m["source_identity"][k] {
                return Err("pg_source_identity");
            }
        }
    }
    if baseline["kind"] != "baseline"
        || transactions["kind"] != "transactions"
        || baseline["source_identity"] != transactions["source_identity"]
        || baseline["catalog"] != transactions["catalog"]
        || baseline["resume_lsn"] != transactions["after_lsn"]
        || transactions["encoding"] != "original_pgoutput_v1_hex_ndjson"
    {
        return Err("pg_boundary");
    }
    let mut tables = BTreeSet::new();
    let mut fields = BTreeSet::new();
    for t in baseline["catalog"].as_array().ok_or("pg_catalog")? {
        let s = text(t, "schema")?;
        let n = text(t, "table")?;
        if !ident(s) || !ident(n) || !tables.insert((s.to_string(), n.to_string())) {
            return Err("pg_table");
        }
        let cols = t["columns"].as_array().ok_or("pg_columns")?;
        let pk = text(t, "primary_key")?;
        if !cols
            .iter()
            .any(|c| c["name"] == pk && c["oid"] == 20 && c["nullable"] == false)
        {
            return Err("pg_key");
        }
        for c in cols {
            if !ident(text(c, "name")?)
                || !matches!(c["oid"].as_i64(), Some(20 | 25))
                || !fields.insert((s.to_string(), n.to_string(), text(c, "name")?.to_string()))
            {
                return Err("pg_type_or_column");
            }
        }
    }
    for config in &configs {
        let approved: BTreeSet<_> = config["approved_objects"]
            .as_array()
            .ok_or("pg_approved")?
            .iter()
            .map(|t| {
                Ok((
                    text(t, "schema")?.to_string(),
                    text(t, "table")?.to_string(),
                ))
            })
            .collect::<Result<_>>()?;
        let approved_fields: BTreeSet<_> = config["approved_fields"]
            .as_array()
            .ok_or("pg_approved")?
            .iter()
            .map(|t| {
                Ok((
                    text(t, "schema")?.to_string(),
                    text(t, "table")?.to_string(),
                    text(t, "column")?.to_string(),
                ))
            })
            .collect::<Result<_>>()?;
        if approved != tables || approved_fields != fields {
            return Err("pg_approved");
        }
    }
    let mut seen = BTreeSet::new();
    let root = bundle.baseline.path.parent().ok_or("pg_path")?;
    for t in baseline["tables"].as_array().ok_or("pg_tables")? {
        let key = (
            text(t, "schema")?.to_string(),
            text(t, "table")?.to_string(),
        );
        if !tables.contains(&key) || !seen.insert(key) || t["complete"] != true {
            return Err("pg_table_set");
        }
        let mut total = 0u64;
        let mut names = BTreeSet::new();
        for p in t["parts"].as_array().ok_or("pg_parts")? {
            if !names.insert(text(p, "path")?) {
                return Err("pg_duplicate_part");
            }
            part(root, p)?.bytes()?;
            total = total
                .checked_add(p["rows"].as_u64().ok_or("pg_rows")?)
                .ok_or("pg_rows")?;
        }
        if t["rows"].as_u64() != Some(total) {
            return Err("pg_rows");
        }
    }
    if seen != tables {
        return Err("pg_table_set");
    }
    let v = Verified {
        bundle,
        baseline,
        transactions,
        source_config: configs[1].clone(),
        digest: pin.sha256.clone(),
    };
    validate_wal(&v)?;
    Ok(v)
}
#[derive(Clone, Debug)]
pub enum Cell {
    Null,
    Unchanged,
    Text(String),
}
impl Cell {
    pub fn json(&self) -> Value {
        match self {
            Self::Null => json!({"kind":"null"}),
            Self::Unchanged => json!({"kind":"unchanged"}),
            Self::Text(s) => json!({"kind":"text","value":s}),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Relation {
    pub oid: u32,
    pub schema: String,
    pub table: String,
    pub columns: Vec<(String, u32, i32)>,
}
#[derive(Debug)]
pub enum Message {
    Begin {
        final_lsn: u64,
        xid: u32,
    },
    Commit {
        commit: u64,
        end: u64,
        time: i64,
    },
    Relation(Relation),
    Event {
        kind: u8,
        oid: u32,
        old: Option<Vec<Cell>>,
        new: Option<Vec<Cell>>,
    },
}
struct Cur<'a> {
    b: &'a [u8],
    i: usize,
}
impl Cur<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.i.checked_add(n).ok_or("pg_wire")?;
        let v = self.b.get(self.i..end).ok_or("pg_wire")?;
        self.i = end;
        Ok(v)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().map_err(|_| "pg_wire")?,
        ))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().map_err(|_| "pg_wire")?,
        ))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().map_err(|_| "pg_wire")?,
        ))
    }
    fn str(&mut self) -> Result<String> {
        let n = self.b[self.i..]
            .iter()
            .position(|b| *b == 0)
            .ok_or("pg_wire")?;
        let s = String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "pg_utf8")?;
        self.take(1)?;
        Ok(s)
    }
    fn tuple(&mut self) -> Result<Vec<Cell>> {
        let n = self.u16()?;
        let mut cells = Vec::new();
        for _ in 0..n {
            cells.push(match self.u8()? {
                b'n' => Cell::Null,
                b'u' => Cell::Unchanged,
                b't' => {
                    let n = self.u32()? as usize;
                    Cell::Text(String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "pg_utf8")?)
                }
                _ => return Err("pg_tuple"),
            });
        }
        Ok(cells)
    }
}
pub fn unhex(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("pg_hex");
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| "pg_hex"))
        .collect()
}
pub fn decode(b: &[u8]) -> Result<Message> {
    let mut c = Cur { b, i: 0 };
    let k = c.u8()?;
    let m = match k {
        b'B' => {
            let final_lsn = c.u64()?;
            c.u64()?;
            Message::Begin {
                final_lsn,
                xid: c.u32()?,
            }
        }
        b'C' => {
            if c.u8()? != 0 {
                return Err("pg_flags");
            }
            Message::Commit {
                commit: c.u64()?,
                end: c.u64()?,
                time: c.u64()? as i64,
            }
        }
        b'R' => {
            let oid = c.u32()?;
            let schema = c.str()?;
            let table = c.str()?;
            c.u8()?;
            let n = c.u16()?;
            let mut columns = Vec::new();
            for _ in 0..n {
                c.u8()?;
                columns.push((c.str()?, c.u32()?, c.u32()? as i32));
            }
            Message::Relation(Relation {
                oid,
                schema,
                table,
                columns,
            })
        }
        b'I' | b'U' | b'D' => {
            let oid = c.u32()?;
            let mut marker = c.u8()?;
            let old = if marker == b'K' || marker == b'O' {
                let v = c.tuple()?;
                if k == b'U' {
                    marker = c.u8()?;
                }
                Some(v)
            } else {
                None
            };
            let new = if k != b'D' {
                if marker != b'N' {
                    return Err("pg_tuple");
                }
                Some(c.tuple()?)
            } else {
                if old.is_none() {
                    return Err("pg_old_key");
                }
                None
            };
            Message::Event {
                kind: k,
                oid,
                old,
                new,
            }
        }
        _ => return Err("pg_message_unsupported"),
    };
    if c.i != b.len() {
        return Err("pg_trailing_wire");
    }
    Ok(m)
}
pub struct Messages {
    parts: std::vec::IntoIter<PathBuf>,
    reader: Option<BufReader<File>>,
}
impl Messages {
    pub fn new(v: &Verified) -> Result<Self> {
        let root = v.bundle.transactions.path.parent().ok_or("pg_path")?;
        let mut paths = Vec::new();
        let mut names = BTreeSet::new();
        for p in v.transactions["parts"].as_array().ok_or("pg_parts")? {
            if !names.insert(text(p, "path")?) {
                return Err("pg_duplicate_part");
            }
            let a = part(root, p)?;
            let bytes = a.bytes()?;
            if bytes.iter().filter(|b| **b == b'\n').count() as u64
                != p["messages"].as_u64().ok_or("pg_messages")?
            {
                return Err("pg_messages");
            }
            paths.push(a.path);
        }
        Ok(Self {
            parts: paths.into_iter(),
            reader: None,
        })
    }
}
impl Iterator for Messages {
    type Item = Result<(String, Message)>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.reader.is_none() {
                let path = self.parts.next()?;
                match File::open(path) {
                    Ok(f) => self.reader = Some(BufReader::new(f)),
                    Err(_) => return Some(Err("pg_part_read")),
                }
            }
            let mut line = String::new();
            match self.reader.as_mut()?.read_line(&mut line) {
                Ok(0) => {
                    self.reader = None;
                    continue;
                }
                Ok(_) => {
                    return Some((|| {
                        let v: Value = serde_json::from_str(&line).map_err(|_| "pg_json")?;
                        Ok((
                            text(&v, "lsn")?.to_string(),
                            decode(&unhex(text(&v, "data_hex")?)?)?,
                        ))
                    })());
                }
                Err(_) => return Some(Err("pg_part_read")),
            }
        }
    }
}
pub fn relations(v: &Verified) -> Result<BTreeMap<u32, Relation>> {
    let mut map = BTreeMap::new();
    for value in v.transactions["relation_messages"]
        .as_object()
        .ok_or("pg_relations")?
        .values()
    {
        if let Message::Relation(r) = decode(&unhex(text(value, "data_hex")?)?)? {
            check_relation(v, &r)?;
            map.insert(r.oid, r);
        } else {
            return Err("pg_relations");
        }
    }
    Ok(map)
}
pub fn check_relation(v: &Verified, r: &Relation) -> Result<()> {
    let shape = v.baseline["catalog"]
        .as_array()
        .ok_or("pg_catalog")?
        .iter()
        .find(|t| t["schema"] == r.schema && t["table"] == r.table)
        .ok_or("pg_unapproved_relation")?;
    let cols = shape["columns"].as_array().ok_or("pg_columns")?;
    if shape["relation_oid"].as_u64() != Some(r.oid.into())
        || cols.len() != r.columns.len()
        || cols
            .iter()
            .zip(&r.columns)
            .any(|(a, (n, o, t))| a["name"] != *n || a["oid"] != *o || a["typmod"] != *t)
    {
        return Err("pg_relation_drift");
    }
    Ok(())
}
fn validate_wal(v: &Verified) -> Result<()> {
    let mut rels = relations(v)?;
    let scopes = v.transactions["transactions"]
        .as_array()
        .filter(|s| !s.is_empty())
        .ok_or("pg_transactions")?;
    let mut index = 0;
    let mut open = None;
    let mut counts = [0u64; 3];
    let mut tables = BTreeSet::new();
    let mut last = lsn(text(&v.baseline, "resume_lsn")?)?;
    for message in Messages::new(v)? {
        let (at, m) = message?;
        match m {
            Message::Begin { final_lsn, xid } => {
                if open.is_some()
                    || final_lsn <= last
                    || final_lsn <= lsn(text(&v.baseline, "observed_wal_lsn")?)?
                {
                    return Err("pg_snapshot_overlap_or_order");
                }
                open = Some((final_lsn, xid, at));
                counts = [0; 3];
                tables.clear();
            }
            Message::Relation(r) => {
                check_relation(v, &r)?;
                rels.insert(r.oid, r);
            }
            Message::Event {
                kind,
                oid,
                old,
                new,
            } => {
                if open.is_none() {
                    return Err("pg_event_boundary");
                }
                let r = rels.get(&oid).ok_or("pg_relation")?;
                for cells in [old, new].into_iter().flatten() {
                    if cells.len() != r.columns.len() {
                        return Err("pg_tuple_shape");
                    }
                }
                counts[if kind == b'I' {
                    0
                } else if kind == b'U' {
                    1
                } else {
                    2
                }] += 1;
                tables.insert((r.schema.clone(), r.table.clone()));
            }
            Message::Commit { commit, end, time } => {
                let (final_lsn, xid, begin) = open.take().ok_or("pg_commit")?;
                let s = scopes.get(index).ok_or("pg_scope_count")?;
                let actual_tables: Value = serde_json::to_value(tables.iter().collect::<Vec<_>>())
                    .map_err(|_| "pg_tables")?;
                if final_lsn != commit
                    || end <= commit
                    || s["complete"] != true
                    || s["xid"] != xid
                    || s["begin_message_lsn"] != begin
                    || lsn(text(s, "begin_final_lsn")?)? != final_lsn
                    || lsn(text(s, "commit_lsn")?)? != commit
                    || lsn(text(s, "end_lsn")?)? != end
                    || s["commit_time_pg_us"] != time
                    || s["inserts"] != counts[0]
                    || s["updates"] != counts[1]
                    || s["deletes"] != counts[2]
                    || s["tables"] != actual_tables
                {
                    return Err("pg_scope_mismatch");
                }
                last = end;
                index += 1;
            }
        }
    }
    if open.is_some() || index != scopes.len() || lsn(text(&v.transactions, "resume_lsn")?)? != last
    {
        return Err("pg_incomplete_capture");
    }
    Ok(())
}
pub fn decision(pin: &Artifact) -> Result<Value> {
    let v = verify(pin)?;
    Ok(
        json!({"protocol":"oo.source-admission.pg-current/v1","admitted":true,"bundle_sha256":pin.sha256,"source_id":v.bundle.source_id,"source_identity":v.baseline["source_identity"],"baseline_sha256":v.bundle.baseline.sha256,"transactions_sha256":v.bundle.transactions.sha256,"end_lsn":v.transactions["resume_lsn"],"transaction_count":v.transactions["transactions"].as_array().ok_or("pg_transactions")?.len()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hex_refuses_unicode_and_odd_input_without_panicking() {
        for s in ["f", "zz", "é", "💣"] {
            assert!(unhex(s).is_err());
        }
        assert_eq!(unhex("00ff").unwrap(), vec![0, 255]);
    }
    #[test]
    fn update_preserves_old_key_and_unchanged_toast() {
        let mut b = vec![b'U'];
        b.extend(7u32.to_be_bytes());
        b.push(b'K');
        b.extend(2u16.to_be_bytes());
        b.push(b't');
        b.extend(1u32.to_be_bytes());
        b.push(b'1');
        b.push(b'n');
        b.push(b'N');
        b.extend(2u16.to_be_bytes());
        b.push(b't');
        b.extend(1u32.to_be_bytes());
        b.push(b'2');
        b.push(b'u');
        match decode(&b).unwrap() {
            Message::Event {
                kind,
                old: Some(old),
                new: Some(new),
                ..
            } => {
                assert_eq!(kind, b'U');
                assert!(matches!(&old[0],Cell::Text(s) if s=="1"));
                assert!(matches!(&new[0],Cell::Text(s) if s=="2"));
                assert!(matches!(new[1], Cell::Unchanged));
            }
            _ => panic!("update"),
        }
        b.push(0);
        assert!(decode(&b).is_err());
    }
    #[test]
    fn exact_lsn_is_not_an_apply_ordinal() {
        assert_eq!(lsn("FFFFFFFF/FFFFFFFF").unwrap(), u64::MAX);
        assert!(lsn("100000000/0").is_err());
        assert!(lsn("1").is_err());
    }
}
