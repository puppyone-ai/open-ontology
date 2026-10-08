//! Load, backup, post-cut mark, restore, and the evidence hash tree.
//!
//! Restore refusal is exit 4 with `replay_ready` false. It does not call `pg_restore`
//! when the manifest or a named hash is wrong.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cut::{self, ContentDigest, RelationDigest};
use crate::decide::{self, publish_offset, Report};
use crate::fixture::{self, SHOP_DDL};
use crate::gold::{self, ACCOUNT_ROWS, ENTRY_ROWS, PARTY_COMMENT, PARTY_ROWS};
use crate::manifest::{self, Manifest, NamedHash};
use crate::pg::{self, Client};
use crate::protocol::{self, copies, schema_path, Operator, AGENT_DB, CONTROL_DB, RESTORE_AGENT_DB, RESTORE_CONTROL_DB};
use crate::sha256::{self, hash_path};
use crate::{fail, Fail};

const USER_OBJECTS: &str = "\
SELECT count(*)::text \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname <> 'pg_catalog' \
  AND n.nspname <> 'information_schema' \
  AND c.relkind IN ('r','p','v','m')";

const CUT_EXPECT: &str = "cut-a356-1|100000";
const WATERMARK_EXPECT: &str = "100000|txn-a356-1|txn-a356-2|100001";
const KEY_EXPECT: &str = "k-0001|member-a|1\nk-0002|member-b|2";

struct Core {
    counts_ok: bool,
    decimal_ok: bool,
    null_ok: bool,
    comment_ok: bool,
    checkpoint_ok: bool,
    key_reused: bool,
    key_count: u64,
    key_identity_ok: bool,
    offset_stored: String,
}

pub fn load_fixture(operator: &Operator) -> Result<(), Fail> {
    prepare_dirs(operator)?;
    let client = open_client(operator)?;
    ensure_roles(&client)?;
    ensure_source_db(&client, CONTROL_DB)?;
    for schema in protocol::SCHEMAS.iter().filter(|item| item.1 == CONTROL_DB) {
        let path = schema_path(&operator.schema_root, schema.2)?;
        client.script(CONTROL_DB, &path)?;
    }
    let cdc = file_hash(operator, protocol::SCHEMAS[0].2)?;
    let session = file_hash(operator, protocol::SCHEMAS[1].2)?;
    let calendar = file_hash(operator, protocol::SCHEMAS[2].2)?;
    write_sql(&operator.work_dir.join("control.sql"), &fixture::control_sql(&cdc, &session, &calendar)?)?;
    client.script(CONTROL_DB, &operator.work_dir.join("control.sql"))?;

    ensure_source_db(&client, AGENT_DB)?;
    let enum_path = schema_path(&operator.schema_root, protocol::SCHEMAS[3].2)?;
    client.script(AGENT_DB, &enum_path)?;
    let enum_hash = file_hash(operator, protocol::SCHEMAS[3].2)?;
    write_sql(&operator.work_dir.join("enum.sql"), &fixture::enum_sql(&enum_hash)?)?;
    client.script(AGENT_DB, &operator.work_dir.join("enum.sql"))?;
    write_sql(&operator.work_dir.join("shop.sql"), SHOP_DDL)?;
    client.script(AGENT_DB, &operator.work_dir.join("shop.sql"))?;
    load_shop(&client, operator)?;
    let core = probe(&client, CONTROL_DB, AGENT_DB, true)?;
    require_core(&core)?;
    let stamp = unix_now()?;
    write_sql(&operator.work_dir.join("last-write.unix"), &format!("{stamp}\n"))?;
    Ok(())
}

pub fn backup(operator: &Operator) -> Result<(), Fail> {
    prepare_dirs(operator)?;
    if operator.backup_dir.join("manifest.json").exists() || operator.backup_dir.join("oo_a356_control.dump").exists() {
        return Err(fail(4, "dump already exists"));
    }
    let client = open_client(operator)?;
    let versions = client.versions()?;
    let started = unix_now()?;
    let mut barrier = cut::Barrier::acquire(&client)?;
    let cut_started_unix = unix_now()?;
    if !cut::probes_rejected(&client)? {
        return Err(fail(1, "writer probe was not rejected"));
    }
    let pairs = barrier.snapshot_pairs();
    let digests_before = cut::content_digests(&client, &pairs)?;
    let lsn_before = scalar(&client, CONTROL_DB, "SELECT pg_current_wal_lsn()::text")?;
    let marker_before = scalar(&client, CONTROL_DB, "SELECT source_cut || '|' || entry_count::text FROM oo_key_fixture.cut_marker")?;
    let count_before = scalar(&client, AGENT_DB, "SELECT count(*)::text FROM shop.entry")?;
    let mut gold = copy_sealed_gold(&client, operator, &barrier)?;
    let mut schemas = Vec::new();
    for schema in protocol::SCHEMAS {
        schemas.push(NamedHash { name: schema.0.to_string(), sha256: file_hash(operator, schema.2)? });
    }
    let control_dump = operator.backup_dir.join("oo_a356_control.dump");
    let agent_dump = operator.backup_dir.join("oo_a356_agent.dump");
    client.dump(CONTROL_DB, &control_dump, barrier.snapshot_id(CONTROL_DB)?)?;
    client.dump(AGENT_DB, &agent_dump, barrier.snapshot_id(AGENT_DB)?)?;
    let digests_after = cut::content_digests(&client, &pairs)?;
    let locks_held = barrier.still_held(&client)?;
    barrier.confirm_sequence_locks(&client)?;
    barrier.reread_sequences()?;
    let cut_finished_unix = unix_now()?;
    let lsn_after = scalar(&client, CONTROL_DB, "SELECT pg_current_wal_lsn()::text")?;
    let marker_after = scalar(&client, CONTROL_DB, "SELECT source_cut || '|' || entry_count::text FROM oo_key_fixture.cut_marker")?;
    let count_after = scalar(&client, AGENT_DB, "SELECT count(*)::text FROM shop.entry")?;
    let signals = cut::Signals {
        marker_stable: marker_before == CUT_EXPECT && marker_after == marker_before,
        count_stable: count_before == "90000" && count_after == count_before,
        locks_held: locks_held && barrier.inventory_matches(),
        probes_rejected: true,
        digests_stable: cut::same_content(&digests_before, &digests_after),
        snapshots_bound: barrier.snapshots_bound(),
        sequence_fence_held: barrier.sequence_fence_held(),
    };
    let cut_proven = cut::proven(&signals);
    let writer_blocks = barrier.blocks();
    let sequence_fence = barrier.sequence_fence();
    let snapshots = barrier.snapshots().to_vec();
    let catalog_body = barrier.catalog_text().to_string();
    barrier.release()?;
    let history_insufficient = !cut_proven;
    let wal_moved = lsn_before != lsn_after;
    let last_write_unix = read_unix(&operator.work_dir.join("last-write.unix"))?;
    let finished = unix_now()?;
    sort_names(&mut gold)?;
    sort_names(&mut schemas)?;
    let relation_digests = bind_digests(&digests_before, &gold)?;
    fs::write(operator.backup_dir.join("catalog.txt"), &catalog_body).map_err(|err| fail(1, format!("catalog: {err}")))?;
    let catalog_sha256 = sha256::hex(&sha256::hash(catalog_body.as_bytes()));
    let manifest = Manifest {
        pg_dump_version: versions.pg_dump,
        pg_restore_version: versions.pg_restore,
        psql_version: versions.psql,
        tool_dir: operator.tool_dir.display().to_string(),
        catalog_sha256,
        cut_finished_unix,
        cut_mode: if cut_proven { cut::WRITERS_BLOCKED.to_string() } else { cut::UNPROVEN.to_string() },
        cut_started_unix,
        history_insufficient,
        wal_moved,
        lsn_before,
        lsn_after,
        entry_count_before: parse_u64(&count_before)?,
        entry_count_after: parse_u64(&count_after)?,
        last_write_unix,
        backup_started_unix: started,
        backup_finished_unix: finished,
        dumps: vec![
            NamedHash { name: "oo_a356_agent.dump".to_string(), sha256: hash_path(&agent_dump)? },
            NamedHash { name: "oo_a356_control.dump".to_string(), sha256: hash_path(&control_dump)? },
        ],
        gold,
        schemas,
        relation_digests,
        sequence_fence,
        snapshots,
        writer_blocks,
        shop_gold_sha256: gold::shop_bundle_sha256(),
    };
    manifest::seal(operator, &manifest)?;
    Ok(())
}

pub fn mark_post_cut(operator: &Operator) -> Result<(), Fail> {
    let _ = manifest::read_sealed(&operator.backup_dir)?;
    let client = open_client(operator)?;
    let party = scalar(&client, AGENT_DB, "SELECT count(*)::text FROM shop.party WHERE party_key = 'p90000'")?;
    let key = scalar(&client, CONTROL_DB, "SELECT count(*)::text FROM oo_key_fixture.issued_key WHERE entity_key = 'k-9000'")?;
    if party != "0" || key != "0" {
        return Err(fail(4, "post-cut row already present"));
    }
    client.sql(
        AGENT_DB,
        "INSERT INTO shop.party (party_key, display_name, active, phone, national_id) VALUES ('p90000', 'post-cut', false, 'tel-post', 'nid-post')",
    )?;
    client.sql(
        CONTROL_DB,
        "INSERT INTO oo_key_fixture.issued_key (entity_key, source_member, issued_generation) VALUES ('k-9000', 'member-post', 3)",
    )?;
    Ok(())
}

pub fn restore(operator: &Operator) -> Result<u8, Fail> {
    prepare_dirs(operator)?;
    let started = unix_now()?;
    let client = open_client(operator)?;
    let manifest = match manifest::read_sealed(&operator.backup_dir) {
        Ok((manifest, _)) => manifest,
        Err(err) if err.exit == 4 => {
            let reason = if err.detail == "manifest_absent" {
                "manifest_absent"
            } else {
                "manifest_hash_mismatch"
            };
            return refuse(operator, reason);
        }
        Err(err) => return Err(err),
    };
    if let Err(err) = cut::proof_shape(&cut::CutProof {
        blocks: &manifest.writer_blocks,
        catalog_sha256: &manifest.catalog_sha256,
        cut_mode: &manifest.cut_mode,
        digests: &manifest.relation_digests,
        fence: &manifest.sequence_fence,
        history_insufficient: manifest.history_insufficient,
        quiescent: !manifest.history_insufficient,
        snapshots: &manifest.snapshots,
    }) {
        if err.exit == 4 {
            return refuse(operator, "cut_unproven");
        }
        return Err(err);
    }
    if let Err(err) = verify_names(operator, &manifest) {
        if err.exit == 4 {
            return refuse(operator, reason_code(&err.detail));
        }
        return Err(err);
    }
    if manifest.history_insufficient || manifest.cut_mode != cut::WRITERS_BLOCKED {
        return refuse(operator, "cut_unproven");
    }
    if let Err(err) = ensure_restore_db(&client, RESTORE_CONTROL_DB) {
        return refuse_or(operator, err);
    }
    if let Err(err) = ensure_restore_db(&client, RESTORE_AGENT_DB) {
        return refuse_or(operator, err);
    }
    if let Err(err) = client.restore_dump(RESTORE_CONTROL_DB, &operator.backup_dir.join("oo_a356_control.dump")) {
        return restore_failed(operator, &err);
    }
    if let Err(err) = client.restore_dump(RESTORE_AGENT_DB, &operator.backup_dir.join("oo_a356_agent.dump")) {
        return restore_failed(operator, &err);
    }
    let compared = match compare_restored(&client, operator) {
        Ok(pair) => pair,
        Err(err) => {
            let _ = fs::write(operator.work_dir.join("tool-error.txt"), format!("{}\n", err.detail));
            (false, false)
        }
    };
    let core = probe(&client, RESTORE_CONTROL_DB, RESTORE_AGENT_DB, false);
    let source_party = scalar(&client, AGENT_DB, "SELECT count(*)::text FROM shop.party").unwrap_or_default();
    let source_keys = scalar(&client, CONTROL_DB, "SELECT count(*)::text FROM oo_key_fixture.issued_key").unwrap_or_default();
    let source_post_party = scalar(&client, AGENT_DB, "SELECT count(*)::text FROM shop.party WHERE party_key = 'p90000'").unwrap_or_default();
    let source_post_key = scalar(&client, CONTROL_DB, "SELECT count(*)::text FROM oo_key_fixture.issued_key WHERE entity_key = 'k-9000'").unwrap_or_default();
    let restore_post_party = scalar(&client, RESTORE_AGENT_DB, "SELECT count(*)::text FROM shop.party WHERE party_key = 'p90000'").unwrap_or_default();
    let restore_post_key = scalar(&client, RESTORE_CONTROL_DB, "SELECT count(*)::text FROM oo_key_fixture.issued_key WHERE entity_key = 'k-9000'").unwrap_or_default();
    let (checkpoint_copy_ok, shop_gold_ok) = compared;
    let core = core.unwrap_or(Core {
        counts_ok: false,
        decimal_ok: false,
        null_ok: false,
        comment_ok: false,
        checkpoint_ok: false,
        key_reused: true,
        key_count: 0,
        key_identity_ok: false,
        offset_stored: String::new(),
    });
    let schemas_ok = schema_hashes_match(&client, &manifest);
    let decision = publish_offset(manifest.history_insufficient, &core.offset_stored, Some("1"));
    let rpo = decide::window_seconds(manifest.last_write_unix, manifest.backup_finished_unix);
    let finished = unix_now();
    let rto = match finished {
        Ok(now) => decide::window_seconds(started, now),
        Err(_) => None,
    };
    let clock_anomaly = rpo.is_none() || rto.is_none();
    let mut report = Report {
        replay_ready: false,
        reason: String::new(),
        reasons: Vec::new(),
        history_insufficient: manifest.history_insufficient,
        cut_proven: manifest.cut_mode == cut::WRITERS_BLOCKED && !manifest.history_insufficient,
        wal_moved: manifest.wal_moved,
        checkpoint_copy_ok: checkpoint_copy_ok && core.checkpoint_ok && schemas_ok,
        shop_gold_ok,
        counts_ok: core.counts_ok,
        decimal_ok: core.decimal_ok,
        null_ok: core.null_ok,
        comment_ok: core.comment_ok,
        key_reused: core.key_reused,
        key_count: if core.key_identity_ok { core.key_count } else { 0 },
        post_cut_absent_on_restore: restore_post_party == "0" && restore_post_key == "0",
        post_cut_present_on_source: source_post_party == "1" && source_post_key == "1" && source_party == "2001" && source_keys == "3",
        clock_anomaly,
        offset_published: decision.published,
        rpo_window_seconds: rpo,
        rto_seconds: rto,
    };
    report.seal();
    emit(operator, &report)?;
    if report.replay_ready { Ok(0) } else { Ok(4) }
}

pub fn hash_tree(root: &Path, out: &Path) -> Result<(), Fail> {
    let root = root.canonicalize().map_err(|err| fail(2, format!("hash tree root: {err}")))?;
    if !root.is_dir() {
        return Err(fail(2, "hash tree root is not a directory"));
    }
    let mut rels = Vec::new();
    walk(&root, &root, &mut rels)?;
    rels.sort();
    let skip_out = match out.canonicalize() {
        Ok(path) => path.strip_prefix(&root).ok().map(rel_string),
        Err(_) => None,
    };
    let skip_side = skip_out.as_ref().map(|name| format!("{name}.sha256"));
    let mut body = String::new();
    for rel in rels {
        if skip_out.as_ref() == Some(&rel) || skip_side.as_ref() == Some(&rel) {
            continue;
        }
        let digest = hash_path(&root.join(&rel))?;
        body.push_str(&digest);
        body.push_str("  ");
        body.push_str(&rel);
        body.push('\n');
    }
    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|err| fail(1, format!("hash tree out: {err}")))?;
    }
    fs::write(out, &body).map_err(|err| fail(1, format!("hash tree out: {err}")))?;
    let digest = sha256::hex(&sha256::hash(body.as_bytes()));
    let side = PathBuf::from(format!("{}.sha256", out.display()));
    fs::write(&side, format!("{digest}\n")).map_err(|err| fail(1, format!("hash tree sidecar: {err}")))?;
    Ok(())
}

fn copy_sealed_gold(client: &Client, operator: &Operator, barrier: &cut::Barrier) -> Result<Vec<NamedHash>, Fail> {
    let gold_dir = operator.backup_dir.join("gold");
    fs::create_dir_all(&gold_dir).map_err(|err| fail(1, format!("gold dir: {err}")))?;
    let mut gold = Vec::new();
    for spec in copies() {
        let path = gold_dir.join(spec.file_name);
        client.copy_in_snapshot(spec.source_db, barrier.snapshot_id(spec.source_db)?, spec.select_sql, &path)?;
        if spec.shop {
            let fresh = operator.work_dir.join("rust-gold").join(spec.file_name);
            write_rust_shop(&fresh, spec.file_name)?;
            require_same(spec.file_name, &path, &fresh)?;
        }
        gold.push(NamedHash { name: spec.file_name.to_string(), sha256: hash_path(&path)? });
    }
    Ok(gold)
}

fn refuse(operator: &Operator, reason: &str) -> Result<u8, Fail> {
    emit(operator, &Report::refused(reason))?;
    Ok(4)
}

fn refuse_or(operator: &Operator, err: Fail) -> Result<u8, Fail> {
    if err.exit == 4 {
        refuse(operator, reason_code(&err.detail))
    } else {
        Err(err)
    }
}

fn restore_failed(operator: &Operator, err: &Fail) -> Result<u8, Fail> {
    let path = operator.work_dir.join("tool-error.txt");
    let _ = fs::write(&path, format!("{}\n", err.detail));
    refuse(operator, "pg_restore_failed")
}

fn reason_code(detail: &str) -> &str {
    if detail == "dump_hash_mismatch"
        || detail == "gold_hash_mismatch"
        || detail == "manifest_shape"
        || detail == "restore_database_not_fresh"
        || detail == "cut_unproven"
    {
        detail
    } else if detail == "manifest_absent" {
        "manifest_absent"
    } else {
        "manifest_hash_mismatch"
    }
}

fn emit(operator: &Operator, report: &Report) -> Result<(), Fail> {
    if let Some(parent) = operator.report_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|err| fail(1, format!("report dir: {err}")))?;
    }
    let body = decide::report_json(report);
    let mut file = File::create(&operator.report_path).map_err(|err| fail(1, format!("report: {err}")))?;
    file.write_all(body.as_bytes()).map_err(|err| fail(1, format!("report: {err}")))?;
    file.sync_all().map_err(|err| fail(1, format!("report: {err}")))?;
    Ok(())
}

fn open_client(operator: &Operator) -> Result<Client, Fail> {
    let client = pg::open(operator)?;
    let _ = client.versions()?;
    Ok(client)
}

fn prepare_dirs(operator: &Operator) -> Result<(), Fail> {
    fs::create_dir_all(&operator.work_dir).map_err(|err| fail(1, format!("work dir: {err}")))?;
    fs::create_dir_all(&operator.backup_dir).map_err(|err| fail(1, format!("backup dir: {err}")))?;
    Ok(())
}

fn ensure_roles(client: &Client) -> Result<(), Fail> {
    for statement in fixture::ROLES {
        let name = fixture::role_name(statement);
        protocol::ident(name)?;
        let count = scalar(client, "postgres", &format!("SELECT count(*)::text FROM pg_roles WHERE rolname = '{name}'"))?;
        if count == "0" {
            client.sql("postgres", statement)?;
        }
    }
    Ok(())
}

fn ensure_source_db(client: &Client, name: &str) -> Result<(), Fail> {
    protocol::ident(name)?;
    if !database_exists(client, name)? {
        client.sql("postgres", &format!("CREATE DATABASE {name} TEMPLATE template0 ENCODING 'UTF8'"))?;
    }
    let objects = scalar(client, name, USER_OBJECTS)?;
    if objects != "0" {
        return Err(fail(4, "source database already has user objects"));
    }
    Ok(())
}

fn ensure_restore_db(client: &Client, name: &str) -> Result<(), Fail> {
    protocol::ident(name)?;
    if !protocol::known_database(name) {
        return Err(fail(2, "database is not authorized"));
    }
    if !database_exists(client, name)? {
        client.sql("postgres", &format!("CREATE DATABASE {name} TEMPLATE template0 ENCODING 'UTF8'"))?;
        return Ok(());
    }
    let objects = scalar(client, name, USER_OBJECTS)?;
    if objects != "0" {
        return Err(fail(4, "restore_database_not_fresh"));
    }
    Ok(())
}

fn database_exists(client: &Client, name: &str) -> Result<bool, Fail> {
    let count = scalar(client, "postgres", &format!("SELECT count(*)::text FROM pg_database WHERE datname = '{name}'"))?;
    Ok(count == "1")
}

fn load_shop(client: &Client, operator: &Operator) -> Result<(), Fail> {
    let dir = operator.work_dir.join("rust-gold");
    fs::create_dir_all(&dir).map_err(|err| fail(1, format!("rust gold: {err}")))?;
    for (file_name, rows, line) in shop_writers() {
        let path = dir.join(file_name);
        let _ = gold::write_table(&path, rows, line)?;
        let table = match file_name {
            "shop-party.copy" => "shop.party (party_key, display_name, active, phone, national_id)",
            "shop-account.copy" => "shop.account (account_id, party_key, opened, label)",
            "shop-entry.copy" => "shop.entry (entry_id, account_id, qty, amount, memo)",
            _ => return Err(fail(1, "unknown shop file")),
        };
        client.copy_from(AGENT_DB, table, &path)?;
    }
    let loaded = operator.work_dir.join("loaded-gold");
    fs::create_dir_all(&loaded).map_err(|err| fail(1, format!("loaded gold: {err}")))?;
    for spec in copies().iter().filter(|spec| spec.shop) {
        let path = loaded.join(spec.file_name);
        client.copy_to(AGENT_DB, spec.select_sql, &path)?;
        require_same(spec.file_name, &dir.join(spec.file_name), &path)?;
    }
    Ok(())
}

fn write_rust_shop(path: &Path, file_name: &str) -> Result<(), Fail> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| fail(1, format!("rust gold: {err}")))?;
    }
    for (name, rows, line) in shop_writers() {
        if name == file_name {
            let _ = gold::write_table(path, rows, line)?;
            return Ok(());
        }
    }
    Err(fail(1, "unknown shop file"))
}

type ShopWriter = (&'static str, i64, fn(i64) -> String);

fn shop_writers() -> [ShopWriter; 3] {
    [
        ("shop-party.copy", PARTY_ROWS, gold::party_line),
        ("shop-account.copy", ACCOUNT_ROWS, gold::account_line),
        ("shop-entry.copy", ENTRY_ROWS, gold::entry_line),
    ]
}

fn compare_restored(client: &Client, operator: &Operator) -> Result<(bool, bool), Fail> {
    let dir = operator.work_dir.join("restore-gold");
    fs::create_dir_all(&dir).map_err(|err| fail(1, format!("restore gold: {err}")))?;
    let mut checkpoint_ok = true;
    let mut shop_ok = true;
    for spec in copies() {
        let path = dir.join(spec.file_name);
        client.copy_to(spec.restore_db, spec.select_sql, &path)?;
        let sealed = operator.backup_dir.join("gold").join(spec.file_name);
        if !same_file(&path, &sealed)? {
            if spec.shop {
                shop_ok = false;
            } else {
                checkpoint_ok = false;
            }
        }
        if spec.shop {
            let fresh = operator.work_dir.join("restore-rust-gold").join(spec.file_name);
            write_rust_shop(&fresh, spec.file_name)?;
            if !same_file(&path, &fresh)? {
                shop_ok = false;
            }
        }
    }
    Ok((checkpoint_ok, shop_ok))
}

fn schema_hashes_match(client: &Client, manifest: &Manifest) -> bool {
    let queries = [
        ("cdc-offset-store-v1", RESTORE_CONTROL_DB, "SELECT sql_sha256 FROM oo_cdc_offset.install_manifest"),
        ("session-store-v1", RESTORE_CONTROL_DB, "SELECT sql_sha256 FROM oo_session_control.install_manifest"),
        ("calendar-store-v1", RESTORE_CONTROL_DB, "SELECT sql_sha256 FROM oo_calendar_control.install_manifest"),
        ("enum-store-v1", RESTORE_AGENT_DB, "SELECT sql_sha256 FROM oo_enum_control.install_manifest"),
    ];
    for (name, database, sql) in queries {
        let Some(item) = manifest.schemas.iter().find(|item| item.name == name) else {
            return false;
        };
        let Ok(got) = scalar(client, database, sql) else {
            return false;
        };
        if got != item.sha256 {
            return false;
        }
    }
    true
}

fn probe(client: &Client, control: &str, agent: &str, hard: bool) -> Result<Core, Fail> {
    let party = scalar(client, agent, "SELECT count(*)::text FROM shop.party");
    let account = scalar(client, agent, "SELECT count(*)::text FROM shop.account");
    let entry = scalar(client, agent, "SELECT count(*)::text FROM shop.entry");
    let orphans = scalar(
        client,
        agent,
        "SELECT (SELECT count(*) FROM shop.account a LEFT JOIN shop.party p ON p.party_key = a.party_key WHERE p.party_key IS NULL) + (SELECT count(*) FROM shop.entry e LEFT JOIN shop.account a ON a.account_id = e.account_id WHERE a.account_id IS NULL)",
    );
    let amounts = scalar(
        client,
        agent,
        "SELECT entry_id::text || '|' || COALESCE(amount::text, '<NULL>') || '|' || qty::text FROM shop.entry WHERE entry_id BETWEEN 0 AND 6 ORDER BY entry_id",
    );
    let null_display = scalar(client, agent, "SELECT CASE WHEN display_name IS NULL THEN 'null' ELSE 'value' END FROM shop.party WHERE party_key = 'p00000'");
    let null_amounts = scalar(client, agent, "SELECT count(*)::text FROM shop.entry WHERE amount IS NULL");
    let comment = scalar(client, agent, "SELECT obj_description('shop.party'::regclass)");
    let watermark = scalar(
        client,
        control,
        "SELECT applied_pos::text || '|' || applied_txn_id || '|' || declared_next_txn || '|' || declared_next_pos::text FROM oo_cdc_offset.watermark",
    );
    let closure = scalar(client, control, "SELECT closure_date || '|' || closure_label FROM oo_calendar_control.snapshot");
    let published = scalar(
        client,
        control,
        "SELECT last_closed_period_end_date::text || '|' || last_closed_period_end_midnight_status FROM oo_calendar_control.published",
    );
    let session = scalar(client, control, "SELECT revision::text FROM oo_session_control.session_head WHERE session_id = 'sess-a356-1'");
    let enumerated = scalar(client, agent, "SELECT code || '|' || status FROM oo_enum_control.published ORDER BY line_no");
    let keys = scalar(
        client,
        control,
        "SELECT entity_key || '|' || source_member || '|' || issued_generation::text FROM oo_key_fixture.issued_key WHERE entity_key IN ('k-0001','k-0002') ORDER BY entity_key",
    );
    let key_members = scalar(client, control, "SELECT entity_key || '|' || source_member FROM oo_key_fixture.issued_key ORDER BY entity_key");
    let key_count = scalar(client, control, "SELECT count(*)::text FROM oo_key_fixture.issued_key WHERE entity_key IN ('k-0001','k-0002')");
    let negative = scalar(
        client,
        control,
        "SELECT count(*)::text FROM oo_key_fixture.negative_decision WHERE left_key = 'k-0001' AND right_key = 'k-0002' AND decision = 'negative'",
    );
    let offset = scalar(client, control, "SELECT applied_pos::text FROM oo_cdc_offset.watermark");
    let party_n = soften("party", party, hard)?;
    let account_n = soften("account", account, hard)?;
    let entry_n = soften("entry", entry, hard)?;
    let orphans = soften("orphans", orphans, hard)?;
    let amounts = soften("amounts", amounts, hard)?;
    let null_display = soften("null-display", null_display, hard)?;
    let null_amounts = soften("null-amounts", null_amounts, hard)?;
    let comment = soften("comment", comment, hard)?;
    let watermark = soften("watermark", watermark, hard)?;
    let closure = soften("closure", closure, hard)?;
    let published = soften("published", published, hard)?;
    let session = soften("session", session, hard)?;
    let enumerated = soften("enum", enumerated, hard)?;
    let key_text = soften("keys", keys, hard)?;
    let members = soften("key-members", key_members, hard)?;
    let key_count = soften("key-count", key_count, hard)?;
    let negative = soften("negative", negative, hard)?;
    let offset_stored = soften("offset", offset, hard)?;
    let counts_ok = party_n == "2000" && account_n == "8000" && entry_n == "90000" && orphans == "0";
    let decimal_ok = amounts == expected_amounts();
    let null_ok = null_display == "null" && null_amounts == gold::null_amount_count().to_string();
    let comment_ok = comment == PARTY_COMMENT;
    let checkpoint_ok = watermark == WATERMARK_EXPECT
        && closure == "2026-03-31|2026-03"
        && published == "2026-03-31|unique"
        && session == "3"
        && enumerated == "confirmed|approved"
        && negative == "1";
    Ok(Core {
        counts_ok,
        decimal_ok,
        null_ok,
        comment_ok,
        checkpoint_ok,
        key_reused: decide::key_reused(&members),
        key_count: key_count.parse().unwrap_or(0),
        key_identity_ok: key_text == KEY_EXPECT,
        offset_stored,
    })
}

fn soften(label: &str, result: Result<String, Fail>, hard: bool) -> Result<String, Fail> {
    match result {
        Ok(value) => Ok(value),
        Err(err) if hard => Err(fail(1, format!("probe {label}: {detail}", detail = err.detail))),
        Err(_) => Ok(String::new()),
    }
}

fn require_core(core: &Core) -> Result<(), Fail> {
    if !core.counts_ok {
        return Err(fail(1, "fixture count mismatch"));
    }
    if !core.decimal_ok {
        return Err(fail(1, "fixture decimal mismatch"));
    }
    if !core.null_ok {
        return Err(fail(1, "fixture null mismatch"));
    }
    if !core.comment_ok {
        return Err(fail(1, "fixture comment mismatch"));
    }
    if !core.checkpoint_ok || !core.key_identity_ok || core.key_reused || core.key_count != 2 || core.offset_stored != "100000" {
        return Err(fail(1, "fixture checkpoint mismatch"));
    }
    Ok(())
}

fn expected_amounts() -> String {
    (0..=6).map(gold::entry_probe).collect::<Vec<_>>().join("\n")
}

fn verify_names(operator: &Operator, manifest: &Manifest) -> Result<(), Fail> {
    if !name_set(&manifest.dumps, &["oo_a356_agent.dump", "oo_a356_control.dump"]) {
        return Err(fail(4, "manifest_shape"));
    }
    let gold_names: Vec<&str> = copies().iter().map(|spec| spec.file_name).collect();
    if !name_set(&manifest.gold, &gold_names) {
        return Err(fail(4, "manifest_shape"));
    }
    let schema_names: Vec<&str> = protocol::SCHEMAS.iter().map(|schema| schema.0).collect();
    if !name_set(&manifest.schemas, &schema_names) {
        return Err(fail(4, "manifest_shape"));
    }
    for item in &manifest.dumps {
        match_hash(&operator.backup_dir.join(&item.name), &item.sha256, "dump_hash_mismatch")?;
    }
    for item in &manifest.gold {
        match_hash(&operator.backup_dir.join("gold").join(&item.name), &item.sha256, "gold_hash_mismatch")?;
    }
    if manifest.shop_gold_sha256 != gold::shop_bundle_sha256() {
        return Err(fail(4, "gold_hash_mismatch"));
    }
    match_hash(&operator.backup_dir.join("catalog.txt"), &manifest.catalog_sha256, "manifest_shape")?;
    let pairs: Vec<(&str, &str)> = manifest.gold.iter().map(|item| (item.name.as_str(), item.sha256.as_str())).collect();
    cut::digests_bound(&manifest.relation_digests, &pairs)?;
    Ok(())
}

fn bind_digests(digests: &[ContentDigest], gold: &[NamedHash]) -> Result<Vec<RelationDigest>, Fail> {
    let mut out = Vec::new();
    for digest in digests {
        let Some(item) = gold.iter().find(|item| item.name == digest.gold_file) else {
            return Err(fail(1, "gold file missing for relation"));
        };
        out.push(RelationDigest {
            content_md5: digest.content_md5.clone(),
            database: digest.database.to_string(),
            gold_sha256: item.sha256.clone(),
            relation: digest.relation.to_string(),
        });
    }
    out.sort_by(|left, right| left.database.cmp(&right.database).then(left.relation.cmp(&right.relation)));
    Ok(out)
}

fn match_hash(path: &Path, expect: &str, code: &str) -> Result<(), Fail> {
    if !path.is_file() {
        return Err(fail(4, "manifest_shape"));
    }
    if hash_path(path)? != expect {
        return Err(fail(4, code));
    }
    Ok(())
}

fn name_set(items: &[NamedHash], expected: &[&str]) -> bool {
    if items.len() != expected.len() {
        return false;
    }
    let mut got: Vec<&str> = items.iter().map(|item| item.name.as_str()).collect();
    let mut want = expected.to_vec();
    got.sort_unstable();
    want.sort_unstable();
    got == want
}

fn sort_names(items: &mut [NamedHash]) -> Result<(), Fail> {
    items.sort_by(|left, right| left.name.cmp(&right.name));
    for pair in items.windows(2) {
        if pair[0].name == pair[1].name {
            return Err(fail(4, "manifest_shape"));
        }
    }
    Ok(())
}

fn file_hash(operator: &Operator, relative: &str) -> Result<String, Fail> {
    hash_path(&schema_path(&operator.schema_root, relative)?)
}

fn write_sql(path: &Path, body: &str) -> Result<(), Fail> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| fail(1, format!("sql dir: {err}")))?;
    }
    fs::write(path, body).map_err(|err| fail(1, format!("write {}: {err}", path.display())))?;
    Ok(())
}

fn scalar(client: &Client, database: &str, sql: &str) -> Result<String, Fail> {
    let text = client.query(database, sql)?;
    Ok(text.trim().to_string())
}

fn parse_u64(text: &str) -> Result<u64, Fail> {
    text.parse().map_err(|_| fail(1, "count is not an integer"))
}

fn read_unix(path: &Path) -> Result<u64, Fail> {
    let text = fs::read_to_string(path).map_err(|err| fail(1, format!("last write: {err}")))?;
    text.trim().parse().map_err(|_| fail(1, "last write is not an integer"))
}

fn unix_now() -> Result<u64, Fail> {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| fail(1, "clock before epoch"))?;
    Ok(duration.as_secs())
}

fn require_same(label: &str, left: &Path, right: &Path) -> Result<(), Fail> {
    if same_file(left, right)? {
        return Ok(());
    }
    let left_text = fs::read(left).unwrap_or_default();
    let right_text = fs::read(right).unwrap_or_default();
    let left_text = String::from_utf8_lossy(&left_text);
    let right_text = String::from_utf8_lossy(&right_text);
    let line = first_diff(&left_text, &right_text);
    Err(fail(1, format!("shop gold mismatch {label} line {line}")))
}

fn same_file(left: &Path, right: &Path) -> Result<bool, Fail> {
    let left = fs::read(left).map_err(|err| fail(1, format!("read {}: {err}", left.display())))?;
    let right = fs::read(right).map_err(|err| fail(1, format!("read {}: {err}", right.display())))?;
    Ok(left == right)
}

fn first_diff(left: &str, right: &str) -> usize {
    for (index, (lhs, rhs)) in left.lines().zip(right.lines()).enumerate() {
        if lhs != rhs {
            return index + 1;
        }
    }
    left.lines().count().max(right.lines().count()) + 1
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), Fail> {
    let entries = fs::read_dir(dir).map_err(|err| fail(1, format!("hash tree: {err}")))?;
    for entry in entries {
        let entry = entry.map_err(|err| fail(1, format!("hash tree: {err}")))?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|err| fail(1, format!("hash tree: {err}")))?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            walk(root, &path, out)?;
        } else if meta.is_file() {
            let rel = path.strip_prefix(root).map_err(|_| fail(1, "hash tree path escaped"))?;
            out.push(rel_string(rel));
        }
    }
    Ok(())
}

fn rel_string(path: &Path) -> String {
    let mut parts = Vec::new();
    for part in path.components() {
        parts.push(part.as_os_str().to_string_lossy().into_owned());
    }
    parts.join("/")
}
