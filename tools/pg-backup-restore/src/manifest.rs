//! Immutable backup manifest. The sidecar is the SHA-256 of the JSON bytes.

use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::cut::{self, RelationDigest, WriterBlock};
use crate::json::{self, escape, Value};
use crate::protocol::{self, Operator};
use crate::sha256::{self, is_hex64};
use crate::snapshot::{SequenceFence, SequenceLock, SnapshotBind};
use crate::{fail, Fail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedHash {
    pub name: String,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub pg_dump_version: String,
    pub pg_restore_version: String,
    pub psql_version: String,
    pub tool_dir: String,
    pub catalog_sha256: String,
    pub cut_finished_unix: u64,
    pub cut_mode: String,
    pub cut_started_unix: u64,
    pub history_insufficient: bool,
    pub wal_moved: bool,
    pub lsn_before: String,
    pub lsn_after: String,
    pub entry_count_before: u64,
    pub entry_count_after: u64,
    pub last_write_unix: u64,
    pub backup_started_unix: u64,
    pub backup_finished_unix: u64,
    pub dumps: Vec<NamedHash>,
    pub gold: Vec<NamedHash>,
    pub schemas: Vec<NamedHash>,
    pub relation_digests: Vec<RelationDigest>,
    pub sequence_fence: SequenceFence,
    pub snapshots: Vec<SnapshotBind>,
    pub writer_blocks: Vec<WriterBlock>,
    pub shop_gold_sha256: String,
}

const KEYS: &[&str] = &[
    "backup_finished_unix",
    "backup_started_unix",
    "caller",
    "catalog_sha256",
    "cut_finished_unix",
    "cut_mode",
    "cut_started_unix",
    "dumps",
    "entry_count_after",
    "entry_count_before",
    "gold",
    "history_insufficient",
    "last_write_unix",
    "lineage_version",
    "lsn_after",
    "lsn_before",
    "manifest_version",
    "owner",
    "pg_dump_version",
    "pg_restore_version",
    "psql_version",
    "quiescent",
    "relation_digests",
    "schemas",
    "sequence_fence",
    "session",
    "shop_gold_sha256",
    "snapshots",
    "source_cut",
    "target_fence",
    "tool_dir",
    "wal_moved",
    "writer_blocks",
];

fn named_json(items: &[NamedHash]) -> String {
    let mut out = String::from("[\n");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(",\n");
        }
        out.push_str("    {\"name\": ");
        out.push_str(&escape(&item.name));
        out.push_str(", \"sha256\": ");
        out.push_str(&escape(&item.sha256));
        out.push('}');
    }
    if !items.is_empty() {
        out.push('\n');
    }
    out.push_str("  ]");
    out
}

pub fn to_json(manifest: &Manifest) -> String {
    format!(
        "{{\n  \"backup_finished_unix\": {finished},\n  \"backup_started_unix\": {started},\n  \"caller\": \"{caller}\",\n  \"catalog_sha256\": {catalog},\n  \"cut_finished_unix\": {cut_finished},\n  \"cut_mode\": {cut_mode},\n  \"cut_started_unix\": {cut_started},\n  \"dumps\": {dumps},\n  \"entry_count_after\": {entry_after},\n  \"entry_count_before\": {entry_before},\n  \"gold\": {gold},\n  \"history_insufficient\": {history},\n  \"last_write_unix\": {last_write},\n  \"lineage_version\": \"{lineage}\",\n  \"lsn_after\": {lsn_after},\n  \"lsn_before\": {lsn_before},\n  \"manifest_version\": \"{manifest_version}\",\n  \"owner\": \"{owner}\",\n  \"pg_dump_version\": {dump_version},\n  \"pg_restore_version\": {restore_version},\n  \"psql_version\": {psql_version},\n  \"quiescent\": {quiescent},\n  \"relation_digests\": {digests},\n  \"schemas\": {schemas},\n  \"sequence_fence\": {sequence_fence},\n  \"session\": \"{session}\",\n  \"shop_gold_sha256\": {shop},\n  \"snapshots\": {snapshots},\n  \"source_cut\": \"{cut}\",\n  \"target_fence\": \"{fence}\",\n  \"tool_dir\": {tool_dir},\n  \"wal_moved\": {wal},\n  \"writer_blocks\": {blocks}\n}}\n",
        finished = manifest.backup_finished_unix,
        started = manifest.backup_started_unix,
        caller = protocol::CALLER,
        catalog = escape(&manifest.catalog_sha256),
        cut_finished = manifest.cut_finished_unix,
        cut_mode = escape(&manifest.cut_mode),
        cut_started = manifest.cut_started_unix,
        dumps = named_json(&manifest.dumps),
        digests = digests_json(&manifest.relation_digests),
        blocks = blocks_json(&manifest.writer_blocks),
        entry_after = manifest.entry_count_after,
        entry_before = manifest.entry_count_before,
        gold = named_json(&manifest.gold),
        history = if manifest.history_insufficient { "true" } else { "false" },
        last_write = manifest.last_write_unix,
        lineage = protocol::LINEAGE,
        lsn_after = escape(&manifest.lsn_after),
        lsn_before = escape(&manifest.lsn_before),
        manifest_version = protocol::MANIFEST_VERSION,
        owner = protocol::OWNER,
        dump_version = escape(&manifest.pg_dump_version),
        restore_version = escape(&manifest.pg_restore_version),
        psql_version = escape(&manifest.psql_version),
        quiescent = if manifest.history_insufficient { "false" } else { "true" },
        schemas = named_json(&manifest.schemas),
        sequence_fence = fence_json(&manifest.sequence_fence),
        session = protocol::SESSION,
        shop = escape(&manifest.shop_gold_sha256),
        snapshots = snapshots_json(&manifest.snapshots),
        cut = protocol::SOURCE_CUT,
        fence = protocol::TARGET_FENCE,
        tool_dir = escape(&manifest.tool_dir),
        wal = if manifest.wal_moved { "true" } else { "false" },
    )
}

pub fn parse(text: &str) -> Result<Manifest, Fail> {
    let value = json::parse(text)?;
    json::require_keys(&value, KEYS)?;
    if value.string("lineage_version")? != protocol::LINEAGE
        || value.string("manifest_version")? != protocol::MANIFEST_VERSION
        || value.string("caller")? != protocol::CALLER
        || value.string("owner")? != protocol::OWNER
        || value.string("session")? != protocol::SESSION
        || value.string("source_cut")? != protocol::SOURCE_CUT
        || value.string("target_fence")? != protocol::TARGET_FENCE
    {
        return Err(fail(4, "manifest identity is not frozen"));
    }
    let history = value.bool_key("history_insufficient")?;
    let quiescent = value.bool_key("quiescent")?;
    let relation_digests = digests(value.array("relation_digests")?)?;
    let writer_blocks = blocks(value.array("writer_blocks")?)?;
    let snapshots = snapshots(value.array("snapshots")?)?;
    let sequence_fence = fence(value.get("sequence_fence").ok_or_else(|| fail(4, "sequence fence is missing"))?)?;
    let catalog_sha256 = value.string("catalog_sha256")?.to_string();
    cut::proof_shape(&cut::CutProof {
        blocks: &writer_blocks,
        catalog_sha256: &catalog_sha256,
        cut_mode: value.string("cut_mode")?,
        digests: &relation_digests,
        fence: &sequence_fence,
        history_insufficient: history,
        quiescent,
        snapshots: &snapshots,
    })?;
    if !is_hex64(value.string("shop_gold_sha256")?) {
        return Err(fail(4, "shop gold digest is not hex"));
    }
    Ok(Manifest {
        catalog_sha256,
        cut_finished_unix: value.u64_key("cut_finished_unix")?,
        cut_mode: value.string("cut_mode")?.to_string(),
        cut_started_unix: value.u64_key("cut_started_unix")?,
        relation_digests,
        sequence_fence,
        snapshots,
        writer_blocks,
        pg_dump_version: value.string("pg_dump_version")?.to_string(),
        pg_restore_version: value.string("pg_restore_version")?.to_string(),
        psql_version: value.string("psql_version")?.to_string(),
        tool_dir: value.string("tool_dir")?.to_string(),
        history_insufficient: history,
        wal_moved: value.bool_key("wal_moved")?,
        lsn_before: value.string("lsn_before")?.to_string(),
        lsn_after: value.string("lsn_after")?.to_string(),
        entry_count_before: value.u64_key("entry_count_before")?,
        entry_count_after: value.u64_key("entry_count_after")?,
        last_write_unix: value.u64_key("last_write_unix")?,
        backup_started_unix: value.u64_key("backup_started_unix")?,
        backup_finished_unix: value.u64_key("backup_finished_unix")?,
        dumps: named(value.array("dumps")?)?,
        gold: named(value.array("gold")?)?,
        schemas: named(value.array("schemas")?)?,
        shop_gold_sha256: value.string("shop_gold_sha256")?.to_string(),
    })
}

fn digests_json(items: &[RelationDigest]) -> String {
    let mut out = String::from("[\n");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(",\n");
        }
        out.push_str("    {\"content_md5\": ");
        out.push_str(&escape(&item.content_md5));
        out.push_str(", \"database\": ");
        out.push_str(&escape(&item.database));
        out.push_str(", \"gold_sha256\": ");
        out.push_str(&escape(&item.gold_sha256));
        out.push_str(", \"relation\": ");
        out.push_str(&escape(&item.relation));
        out.push('}');
    }
    if !items.is_empty() {
        out.push('\n');
    }
    out.push_str("  ]");
    out
}

fn blocks_json(items: &[WriterBlock]) -> String {
    let mut out = String::from("[\n");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(",\n");
        }
        out.push_str("    {\"application_name\": ");
        out.push_str(&escape(&item.application_name));
        out.push_str(", \"database\": ");
        out.push_str(&escape(&item.database));
        out.push_str(", \"lock_mode\": ");
        out.push_str(&escape(&item.lock_mode));
        out.push_str(", \"probe_rejected\": ");
        out.push_str(if item.probe_rejected { "true" } else { "false" });
        out.push_str(", \"relations\": ");
        out.push_str(&string_array(&item.relations));
        out.push('}');
    }
    if !items.is_empty() {
        out.push('\n');
    }
    out.push_str("  ]");
    out
}

fn fence_json(fence: &SequenceFence) -> String {
    let mut sequences = String::from("[\n");
    for (index, item) in fence.sequences.iter().enumerate() {
        if index > 0 {
            sequences.push_str(",\n");
        }
        sequences.push_str("      {\"database\": ");
        sequences.push_str(&escape(&item.database));
        sequences.push_str(", \"held\": ");
        sequences.push_str(if item.held { "true" } else { "false" });
        sequences.push_str(", \"relation\": ");
        sequences.push_str(&escape(&item.relation));
        sequences.push_str(", \"sqlstate\": ");
        sequences.push_str(&escape(&item.sqlstate));
        sequences.push('}');
    }
    if !fence.sequences.is_empty() {
        sequences.push('\n');
    }
    sequences.push_str("    ]");
    format!(
        "{{\n    \"held\": {held},\n    \"lock_mode\": {mode},\n    \"method\": {method},\n    \"sequences\": {sequences}\n  }}",
        held = if fence.held { "true" } else { "false" },
        mode = escape(&fence.lock_mode),
        method = escape(&fence.method),
        sequences = sequences.as_str(),
    )
}

fn snapshots_json(items: &[SnapshotBind]) -> String {
    let mut out = String::from("[\n");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(",\n");
        }
        out.push_str("    {\"database\": ");
        out.push_str(&escape(&item.database));
        out.push_str(", \"id\": ");
        out.push_str(&escape(&item.id));
        out.push_str(", \"isolation\": ");
        out.push_str(&escape(&item.isolation));
        out.push('}');
    }
    if !items.is_empty() {
        out.push('\n');
    }
    out.push_str("  ]");
    out
}

fn string_array(items: &[String]) -> String {
    let mut out = String::from("[");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&escape(item));
    }
    out.push(']');
    out
}

fn digests(items: &[Value]) -> Result<Vec<RelationDigest>, Fail> {
    let mut out = Vec::new();
    for item in items {
        json::require_keys(item, &["content_md5", "database", "gold_sha256", "relation"])?;
        out.push(RelationDigest {
            content_md5: item.string("content_md5")?.to_string(),
            database: item.string("database")?.to_string(),
            gold_sha256: item.string("gold_sha256")?.to_string(),
            relation: item.string("relation")?.to_string(),
        });
    }
    Ok(out)
}

fn blocks(items: &[Value]) -> Result<Vec<WriterBlock>, Fail> {
    let mut out = Vec::new();
    for item in items {
        json::require_keys(item, &["application_name", "database", "lock_mode", "probe_rejected", "relations"])?;
        out.push(WriterBlock {
            application_name: item.string("application_name")?.to_string(),
            database: item.string("database")?.to_string(),
            lock_mode: item.string("lock_mode")?.to_string(),
            probe_rejected: item.bool_key("probe_rejected")?,
            relations: string_list(item.array("relations")?)?,
        });
    }
    Ok(out)
}

fn snapshots(items: &[Value]) -> Result<Vec<SnapshotBind>, Fail> {
    let mut out = Vec::new();
    for item in items {
        json::require_keys(item, &["database", "id", "isolation"])?;
        out.push(SnapshotBind {
            database: item.string("database")?.to_string(),
            id: item.string("id")?.to_string(),
            isolation: item.string("isolation")?.to_string(),
        });
    }
    Ok(out)
}

fn fence(value: &Value) -> Result<SequenceFence, Fail> {
    json::require_keys(value, &["held", "lock_mode", "method", "sequences"])?;
    let mut sequences = Vec::new();
    for item in value.array("sequences")? {
        json::require_keys(item, &["database", "held", "relation", "sqlstate"])?;
        sequences.push(SequenceLock {
            database: item.string("database")?.to_string(),
            held: item.bool_key("held")?,
            relation: item.string("relation")?.to_string(),
            sqlstate: item.string("sqlstate")?.to_string(),
        });
    }
    Ok(SequenceFence {
        held: value.bool_key("held")?,
        lock_mode: value.string("lock_mode")?.to_string(),
        method: value.string("method")?.to_string(),
        sequences,
    })
}

fn string_list(items: &[Value]) -> Result<Vec<String>, Fail> {
    let mut out = Vec::new();
    for item in items {
        let Value::String(text) = item else {
            return Err(fail(4, "relation name is not a string"));
        };
        out.push(text.clone());
    }
    Ok(out)
}

fn named(items: &[Value]) -> Result<Vec<NamedHash>, Fail> {
    let mut out = Vec::new();
    for item in items {
        json::require_keys(item, &["name", "sha256"])?;
        let digest = item.string("sha256")?;
        if !is_hex64(digest) {
            return Err(fail(4, "manifest digest is not hex"));
        }
        let name = item.string("name")?;
        if name.contains('/') || name.contains("..") || name.is_empty() {
            return Err(fail(4, "manifest file name is refused"));
        }
        out.push(NamedHash { name: name.to_string(), sha256: digest.to_string() });
    }
    Ok(out)
}

pub fn seal(operator: &Operator, manifest: &Manifest) -> Result<(), Fail> {
    let path = operator.backup_dir.join("manifest.json");
    let side = operator.backup_dir.join("manifest.sha256");
    if path.exists() || side.exists() {
        return Err(fail(4, "manifest already exists"));
    }
    let body = to_json(manifest);
    if parse(&body).is_err() {
        return Err(fail(1, "manifest writer produced unreadable json"));
    }
    {
        let mut file = File::create(&path).map_err(|err| fail(1, format!("manifest: {err}")))?;
        file.write_all(body.as_bytes()).map_err(|err| fail(1, format!("manifest: {err}")))?;
        file.sync_all().map_err(|err| fail(1, format!("manifest: {err}")))?;
    }
    let digest = sha256::hex(&sha256::hash(body.as_bytes()));
    {
        let mut file = File::create(&side).map_err(|err| fail(1, format!("manifest sidecar: {err}")))?;
        writeln!(file, "{digest}").map_err(|err| fail(1, format!("manifest sidecar: {err}")))?;
        file.sync_all().map_err(|err| fail(1, format!("manifest sidecar: {err}")))?;
    }
    readonly(&path)?;
    readonly(&side)?;
    Ok(())
}

fn readonly(path: &Path) -> Result<(), Fail> {
    let mut permissions = fs::metadata(path).map_err(|err| fail(1, format!("manifest mode: {err}")))?.permissions();
    permissions.set_mode(0o444);
    fs::set_permissions(path, permissions).map_err(|err| fail(1, format!("manifest mode: {err}")))?;
    Ok(())
}

pub fn read_sealed(dir: &Path) -> Result<(Manifest, String), Fail> {
    let path = dir.join("manifest.json");
    let side = dir.join("manifest.sha256");
    if !path.is_file() || !side.is_file() {
        return Err(fail(4, "manifest_absent"));
    }
    let body = fs::read(&path).map_err(|err| fail(4, format!("manifest read: {err}")))?;
    let side_text = fs::read_to_string(&side).map_err(|err| fail(4, format!("manifest sidecar: {err}")))?;
    let want = side_text.split_whitespace().next().unwrap_or("");
    let got = sha256::hex(&sha256::hash(&body));
    if want != got {
        return Err(fail(4, "manifest_hash_mismatch"));
    }
    let text = String::from_utf8(body).map_err(|_| fail(4, "manifest is not utf-8"))?;
    let manifest = parse(&text).map_err(|err| fail(4, format!("manifest_hash_mismatch: {}", err.detail)))?;
    Ok((manifest, got))
}
