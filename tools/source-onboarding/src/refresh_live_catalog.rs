//! Replace only an already reviewed library's published cut, never its authority.
use crate::io::*;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

const HELP: &str = r#"Usage: source-onboarding refresh-live-catalog --config PRIVATE_CONFIG --out PRIVATE_DIR [--dry-run]

PRIVATE_CONFIG is an absolute, owner-only JSON file:
{
  "protocol": "oo.library.refresh-live-catalog/v1",
  "gateway_root": "/absolute/existing/runtime",
  "publication_receipt": "/absolute/refresh-publish/publication.json",
  "expected_pack_digest": "<64 lowercase hex>",
  "expected_manifest_sha256": "<Pack manifest.json raw-text SHA256>",
  "schema_scope_digest": "<SHA256 of compact JSON sorted dataset names>",
  "metadata_argv": ["/absolute/path/psql", "-h", "127.0.0.1",
                    "-p", "5432", "-U", "metadata_reader", "-d", "target"],
  "cli_binary": {"path": "/absolute/pinned/oo", "sha256": "<SHA256>"},
  "gateway": "https://gateway.example",
  "token_file": "/absolute/private/existing-token",
  "queries": [{"sql": "SELECT count(*) FROM app.items", "expected_rows": [["42"]]}]
}
Optional config keys: ca_file, client_pem, connect_ip, interface.
TLS/token files must be absolute, owner-only regular files. Credentials belong
in files/environment (e.g. PGPASSFILE), never metadata_argv or connection URIs.
metadata_argv accepts only psql connection flags -h/-p/-U/-d (or long forms),
-X/-q/-A/-t/-w/-qAt and -v ON_ERROR_STOP=1. No shell, command or script flags.
Explicit host, port and database must match the runtime data endpoint.
The command adds -X -qAt -w -v ON_ERROR_STOP=1 and runs only fixed read-only SQL.

Requires an existing complete reviewed/library/library.json and unchanged Pack,
OOS, review, visibility, credentials and endpoints. The publication receipt must
be the refresh_publish {prepared:{native_mapping:{publication:...}},release:...}
artifact: knowledge_published=true, matching Pack and native receipt hash.
Currently supports immutable per_table library snapshots, not transactional CDC.
The exact ready library head, membership and each sealed published run/table
are checked; mutable table_freshness is never the publication cut.

--dry-run performs the same receipt/head/six-field/scope/Pack/CAS prechecks,
retains prepared.json and candidate-catalog.json privately, but does not replace
the catalog or require post-switch queries. It is not activation authorization:
the actual invocation always rechecks. No Gateway is started by either mode.

Actual activation uses a runtime-wide OS lock, byte/inode CAS, private same-dir
temp + fsync + rename + directory fsync. Only publication/bindings change.
Same-receipt replay is a no-write verification, not a restart or rollback.
After activation the pinned standard CLI runs freshness --summary, exact
manifest, validate and query under one publication binding. Failures are retained
per attempt; a failed post-check NEVER rolls back the target or the catalog.
Each attempt has started.json and (on normal exit) result.json; no result means
incomplete, not success. A renamed.json records a completed rename separately.
Exit 0: prepared (--dry-run), verified activation or verified no-op. Exit 2: error.
Success JSON includes success:true, pack_digest and schema_scope_digest.
Only a verified actual invocation includes publication_binding (F22 id + vector);
dry-run does not claim a verified Gateway publication binding.
Output describes a published source snapshot, NOT source-real-time freshness.
"#;

pub fn dispatch(args: &[String]) -> Result<()> {
    if args == ["--help"] || args == ["-h"] {
        println!("{HELP}");
        return Ok(());
    }
    require(
        (args.len() == 4 || args.len() == 5 && args[4] == "--dry-run")
            && args[0] == "--config"
            && args[2] == "--out",
        "usage: refresh-live-catalog --config PRIVATE_CONFIG --out PRIVATE_DIR [--dry-run]",
    )?;
    println!(
        "{}",
        run(Path::new(&args[1]), Path::new(&args[3]), args.len() == 5)?
    );
    Ok(())
}

fn hex(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn nonempty<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    let s = text(v, key)?;
    require(
        !s.is_empty() && !s.chars().any(char::is_control),
        "live_catalog_text",
    )?;
    Ok(s)
}

fn parse(raw: &[u8]) -> Result<Value> {
    oo_core::json_scan::parse_json_value_with_limits(
        std::str::from_utf8(raw)?,
        "oo.library.refresh-live-catalog/v1",
        &oo_core::json_scan::JsonScanLimits {
            max_bytes: 64 * 1024 * 1024,
            max_nodes: 4_000_000,
            max_depth: 128,
        },
    )
    .map_err(|_| "live_catalog_json_invalid_or_duplicate".into())
}

fn directory(path: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    require(
        path.is_absolute()
            && path.canonicalize()? == path
            && m.is_dir()
            && !m.file_type().is_symlink()
            && m.mode() & 0o077 == 0,
        "live_catalog_directory",
    )
}

fn private_file(path: &Path) -> Result<()> {
    private(path)?;
    require(
        path.canonicalize()? == path && fs::metadata(path)?.nlink() == 1,
        "live_catalog_file_alias",
    )
}

fn names(value: &Value, key: &str, field: &str) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for entry in array(value, key)? {
        require(
            names.insert(nonempty(entry, field)?.to_owned()),
            "live_catalog_duplicate_dataset",
        )?;
    }
    require(!names.is_empty(), "live_catalog_empty_scope")?;
    Ok(names)
}

fn membership(publication: &Value) -> Result<BTreeSet<String>> {
    let receipt = &publication["receipt"];
    let library = &receipt["library"];
    let manifest = &library["manifest"];
    require(
        publication["protocol"] == "oo.pipeline.native-library-publication/v1"
            && library["protocol"] == publication["protocol"]
            && publication["receipt_sha256"] == sha(&bytes(receipt)?)
            && manifest["protocol"] == "oo.library.membership/v1"
            && library["manifest_sha256"] == sha(&bytes(manifest)?)
            && nonempty(library, "library_id")? == nonempty(manifest, "library_id")?,
        "live_catalog_publication_hash",
    )?;
    let shards = array(receipt, "shards")?;
    let sources = array(manifest, "sources")?;
    require(
        shards.len() == sources.len() && shards.len() == array(library, "shards")?.len(),
        "live_catalog_membership",
    )?;
    let mut seen_sources = BTreeSet::new();
    let mut datasets = BTreeSet::new();
    let mut anchor_seen = false;
    for member in sources {
        let source = nonempty(member, "source_id")?;
        require(
            seen_sources.insert(source)
                && array(library, "shards")?
                    .iter()
                    .filter(|v| **v == source)
                    .count()
                    == 1,
            "live_catalog_membership",
        )?;
        let matches: Vec<_> = shards.iter().filter(|s| s["source_id"] == source).collect();
        require(matches.len() == 1, "live_catalog_membership")?;
        let shard = matches[0];
        let r = &shard["receipt"];
        require(
            shard["receipt_sha256"] == sha(&bytes(r)?)
                && r["protocol"] == "oo.pipeline.native-publication-receipt/v1"
                && r["source_id"] == source
                && r["run_id"] == shard["anchor_run"],
            "live_catalog_shard_hash",
        )?;
        // C1 has a different mutable/current-cut contract. Never downgrade it to a snapshot.
        require(
            r["consistency"] == "per_table",
            "live_catalog_requires_per_table_snapshot",
        )?;
        let declared = array(member, "tables")?;
        let tables = array(r, "tables")?;
        let mut runs = BTreeSet::new();
        for run in array(r, "run_ids")? {
            require(
                runs.insert(run.as_str().ok_or("live_catalog_run_id")?),
                "live_catalog_run_id",
            )?;
        }
        require(
            declared.len() == tables.len()
                && !tables.is_empty()
                && runs.contains(nonempty(shard, "anchor_run")?),
            "live_catalog_membership",
        )?;
        let mut table_runs = BTreeSet::new();
        for t in tables {
            let schema = t
                .get("storage_schema")
                .unwrap_or(&t["schema"])
                .as_str()
                .ok_or("live_catalog_schema")?;
            let table = t
                .get("storage_table")
                .unwrap_or(&t["table"])
                .as_str()
                .ok_or("live_catalog_table")?;
            require(
                member["schema"] == schema
                    && declared.iter().filter(|n| **n == table).count() == 1
                    && datasets.insert(format!("{schema}.{table}"))
                    && ["source_generation", "target_generation"]
                        .iter()
                        .all(|k| t[*k].as_i64().is_some_and(|n| n > 0))
                    && t["accepted_rows"].as_u64().is_some()
                    && hex(&t["canonical_digest"])
                    && runs.contains(nonempty(t, "run_id")?),
                "live_catalog_membership",
            )?;
            table_runs.insert(nonempty(t, "run_id")?);
            nonempty(t, "physical_name")?;
            nonempty(t, "applied_pos")?;
        }
        require(runs == table_runs, "live_catalog_run_scope")?;
        anchor_seen |= shard["anchor_run"] == publication["run_id"];
    }
    require(
        anchor_seen
            && !datasets.is_empty()
            && library["expected_tables"].as_u64() == Some(datasets.len() as u64),
        "live_catalog_membership",
    )?;
    Ok(datasets)
}

fn check_pack(c: &Value, catalog: &Value, root: &Path) -> Result<()> {
    let producer = &catalog["envelope"]["producer"];
    let artifact = &producer["artifact"];
    require(
        catalog["protocol"] == "oo.gateway.library-reviewed/v1"
            && producer["pack_digest"] == c["expected_pack_digest"]
            && artifact["pack_digest"] == c["expected_pack_digest"]
            && artifact["manifest_sha256"] == c["expected_manifest_sha256"]
            && producer["effective_sha256"] == sha(&bytes(&catalog["effective"])?)
            && producer["visibility_sha256"] == sha(&bytes(&catalog["visibility"])?)
            && catalog["envelope"]["output_sha256"] == sha(&bytes(producer)?),
        "live_catalog_pack",
    )?;
    if catalog["envelope"]["contract"] == "oo.operator.library-pack/v1" {
        require(
            producer["review_sha256"] == sha(&bytes(&catalog["envelope"]["review"])?),
            "live_catalog_review",
        )?;
    }
    let mut files = BTreeMap::new();
    for f in array(artifact, "files")? {
        let body = text(f, "text")?;
        require(
            f["sha256"] == sha(body.as_bytes())
                && files.insert(nonempty(f, "path")?, body).is_none(),
            "live_catalog_pack_file",
        )?;
    }
    let records: Vec<_> = files
        .iter()
        .map(|(path, body)| {
            json!({
        "path":path,"bytes":body.len(),"sha256":sha(body.as_bytes())})
        })
        .collect();
    require(
        sha(&bytes(&json!(records))?) == c["expected_pack_digest"]
            && files
                .get("manifest.json")
                .is_some_and(|s| sha(s.as_bytes()) == c["expected_manifest_sha256"]),
        "live_catalog_pack_digest",
    )?;
    let mut matches = 0;
    for entry in fs::read_dir(root.join("packs"))? {
        let path = entry?.path();
        private_file(&path)?;
        let installed = load(&path)?;
        if installed["pack_digest"] == c["expected_pack_digest"] {
            require(installed == *artifact, "live_catalog_installed_pack")?;
            matches += 1;
        }
    }
    require(matches == 1, "live_catalog_installed_pack")
}

fn metadata_argv(c: &Value) -> Result<Vec<String>> {
    let argv = array(c, "metadata_argv")?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or("live_catalog_metadata_argv")
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    require(
        argv.first().is_some_and(|p| {
            Path::new(p).is_absolute() && Path::new(p).file_name().is_some_and(|n| n == "psql")
        }),
        "live_catalog_metadata_psql",
    )?;
    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "-X" | "-q" | "-A" | "-t" | "-w" | "-qAt" => i += 1,
            "-h" | "--host" | "-p" | "--port" | "-U" | "--username" | "-d" | "--dbname" => {
                let v = argv.get(i + 1).ok_or("live_catalog_metadata_argv")?;
                require(
                    !v.is_empty()
                        && !v.starts_with('-')
                        && !v
                            .chars()
                            .any(|ch| ch.is_whitespace() || "=@\\'\"".contains(ch))
                        && !v.contains("://"),
                    "live_catalog_metadata_no_credentials",
                )?;
                i += 2;
            }
            "-v" if argv.get(i + 1).is_some_and(|v| v == "ON_ERROR_STOP=1") => i += 2,
            _ => return Err("live_catalog_metadata_read_only_flags".into()),
        }
    }
    Ok(argv)
}

fn metadata(c: &Value, p: &Value, role: &str, calls: &mut Calls, label: &str) -> Result<Value> {
    let encoded = String::from_utf8(bytes(&json!({
        "id":p["receipt"]["library"]["library_id"],"sha":p["receipt_sha256"],
        "shards":p["receipt"]["shards"],"pack":c["expected_pack_digest"],"role":role}))?)?
    .replace('\'', "''");
    let sql = format!(
        r#"BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL statement_timeout='15s';
SET LOCAL lock_timeout='5s';
SET LOCAL standard_conforming_strings=on;
WITH input AS (SELECT '{encoded}'::jsonb v),
wanted AS (SELECT s->>'source_id' source, t
 FROM input CROSS JOIN LATERAL jsonb_array_elements(v->'shards') s
 CROSS JOIN LATERAL jsonb_array_elements(s->'receipt'->'tables') t)
SELECT jsonb_build_object(
 'head',(SELECT jsonb_build_object('library_id',h.library_id,'state',h.state,
    'receipt_sha256',h.receipt_sha256,'manifest_sha256',h.manifest_sha256)
    FROM _oo.native_library_head h,input WHERE h.library_id=v->>'id'),
 'receipt',(SELECT jsonb_build_object('library',p.library,'shards',p.shards)
    FROM _oo.native_library_publication p,input
    WHERE p.library_id=v->>'id' AND p.receipt_sha256=v->>'sha'),
 'rows',(SELECT jsonb_agg(jsonb_build_object('source',w.source,'table',w.t,
    'apply_match',EXISTS(SELECT 1 FROM _oo.apply_receipts a WHERE a.source=r.source_id
      AND a.table_generation=t.physical_name AND a.config_digest=r.config_digest
      AND a.schema_digest=r.schema_digest AND a.type_digest=r.type_digest
      AND a.fence_generation=r.target_generation AND a.fence_shard=r.shard_id AND NOT a.partial_txn),
    'fence_match',EXISTS(SELECT 1 FROM oo_publish_target.fence f,input
      WHERE f.role_name=v->>'role' AND f.schema_name='_oo' AND f.relation_name=t.physical_name
      AND f.state='open' AND f.opened_pack_digest=v->>'pack'),
    'run',jsonb_build_object('run_id',r.run_id,'source_id',r.source_id,
      'source_generation',r.source_generation,'target_generation',r.target_generation,
      'config_digest',r.config_digest,'type_digest',r.type_digest,'input_digest',r.input_digest,
      'schema_digest',r.schema_digest,'state',r.state,'sealed',r.sealed_at IS NOT NULL),
    'import_table',jsonb_build_object('run_id',t.run_id,'schema_ident',t.schema_ident,
      'table_ident',t.table_ident,'physical_name',t.physical_name,'exhausted',t.exhausted,
      'applied_rows',t.applied_rows,'applied_pos',t.applied_pos::text,
      'canonical_digest',t.canonical_digest)))
 FROM wanted w LEFT JOIN _oo.full_import_run r
 ON r.run_id::text=w.t->>'run_id' AND r.source_id=w.source
 AND r.source_generation=(w.t->>'source_generation')::bigint
 AND r.target_generation=(w.t->>'target_generation')::bigint
 LEFT JOIN _oo.full_import_table t ON t.run_id=r.run_id
 AND t.schema_ident=COALESCE(w.t->>'storage_schema',w.t->>'schema')
 AND t.table_ident=COALESCE(w.t->>'storage_table',w.t->>'table')
 AND t.physical_name=w.t->>'physical_name'));
ROLLBACK;
"#
    );
    let mut argv = metadata_argv(c)?;
    let root = Path::new(text(c, "gateway_root")?);
    let runtime = load(root.join("config.json"))?;
    let endpoint = load(root.join(text(&runtime, "data_endpoint_file")?))?;
    for (flags, value) in [
        (["-h", "--host"], nonempty(&endpoint, "host")?.to_owned()),
        (
            ["-p", "--port"],
            endpoint["port"]
                .as_u64()
                .ok_or("live_catalog_metadata_target")?
                .to_string(),
        ),
        (
            ["-d", "--dbname"],
            nonempty(&endpoint, "database")?.to_owned(),
        ),
    ] {
        let values: Vec<_> = argv
            .windows(2)
            .filter(|a| flags.contains(&a[0].as_str()))
            .collect();
        require(
            values.len() == 1 && values[0][1] == value,
            "live_catalog_metadata_target",
        )?;
    }
    argv.extend(strings(&[
        "-X",
        "-qAt",
        "-w",
        "-v",
        "ON_ERROR_STOP=1",
        "-f",
        "-",
    ]));
    Ok(serde_json::from_slice(&calls.run(
        label,
        &argv,
        Some(sql.as_bytes()),
    )?)?)
}

fn bindings(p: &Value, m: &Value) -> Result<Value> {
    let library = &p["receipt"]["library"];
    require(
        m["head"]["library_id"] == library["library_id"]
            && m["head"]["state"] == "ready"
            && m["head"]["receipt_sha256"] == p["receipt_sha256"]
            && m["head"]["manifest_sha256"] == library["manifest_sha256"],
        "live_catalog_wrong_head",
    )?;
    require(
        m["receipt"] == p["receipt"] && sha(&bytes(&m["receipt"])?) == p["receipt_sha256"],
        "live_catalog_target_receipt",
    )?;
    let mut expected = BTreeMap::new();
    for shard in array(&p["receipt"], "shards")? {
        for t in array(&shard["receipt"], "tables")? {
            let schema = t.get("storage_schema").unwrap_or(&t["schema"]);
            let table = t.get("storage_table").unwrap_or(&t["table"]);
            let dataset = format!(
                "{}.{}",
                schema.as_str().ok_or("live_catalog_schema")?,
                table.as_str().ok_or("live_catalog_table")?
            );
            require(
                expected
                    .insert(dataset, (shard["source_id"].clone(), t, schema, table))
                    .is_none(),
                "live_catalog_duplicate_dataset",
            )?;
        }
    }
    let mut result = BTreeMap::new();
    for row in array(m, "rows")? {
        let r = &row["run"];
        let t = &row["import_table"];
        let dataset = format!(
            "{}.{}",
            nonempty(t, "schema_ident")?,
            nonempty(t, "table_ident")?
        );
        let (source, published, schema, table) =
            expected.get(&dataset).ok_or("live_catalog_run_scope")?;
        require(
            row["source"] == *source
                && row["table"] == **published
                && r["source_id"] == *source
                && r["run_id"] == published["run_id"]
                && r["source_generation"] == published["source_generation"]
                && r["target_generation"] == published["target_generation"]
                && r["sealed"] == true
                && r["state"] == "published"
                && row["apply_match"] == true
                && [
                    "config_digest",
                    "type_digest",
                    "input_digest",
                    "schema_digest",
                ]
                .iter()
                .all(|k| hex(&r[*k]))
                && t["run_id"] == r["run_id"]
                && t["schema_ident"] == **schema
                && t["table_ident"] == **table
                && t["physical_name"] == published["physical_name"]
                && t["exhausted"] == true
                && t["applied_rows"] == published["accepted_rows"]
                && t["canonical_digest"] == published["canonical_digest"]
                && t["applied_pos"] == published["applied_pos"],
            "live_catalog_immutable_run_table",
        )?;
        require(row["fence_match"] == true, "live_catalog_target_fence_pack")?;
        let binding = json!({"dataset":dataset,"source":r["source_id"],
            "config_digest":r["config_digest"],"generation":r["source_generation"],
            "type_digest":r["type_digest"],"input_digest":r["input_digest"]});
        require(
            result.insert(dataset, binding).is_none(),
            "live_catalog_ambiguous_run_table",
        )?;
    }
    require(result.keys().eq(expected.keys()), "live_catalog_run_scope")?;
    Ok(json!(result.into_values().collect::<Vec<_>>()))
}

#[derive(PartialEq)]
struct Snapshot {
    bytes: Vec<u8>,
    identity: (u64, u64, i64, i64, i64, i64),
}

fn snapshot(path: &Path) -> Result<Snapshot> {
    private_file(path)?;
    let f = File::open(path)?;
    let before = f.metadata()?;
    let content = fs::read(path)?;
    let after = fs::symlink_metadata(path)?;
    let identity = |m: &fs::Metadata| {
        (
            m.dev(),
            m.ino(),
            m.ctime(),
            m.ctime_nsec(),
            m.mtime(),
            m.mtime_nsec(),
        )
    };
    require(
        identity(&before) == identity(&after) && before.len() == content.len() as u64,
        "live_catalog_cas",
    )?;
    Ok(Snapshot {
        bytes: content,
        identity: identity(&after),
    })
}

fn cas(path: &Path, original: &Snapshot) -> Result<()> {
    require(snapshot(path)? == *original, "live_catalog_cas")
}

fn runtime_seal(c: &Value, root: &Path) -> Result<Value> {
    fn collect(path: &Path, skip: &Path, files: &mut BTreeMap<String, String>) -> Result<()> {
        if path == skip {
            return Ok(());
        }
        let meta = fs::symlink_metadata(path)?;
        if meta.is_dir() {
            directory(path)?;
            for e in fs::read_dir(path)? {
                collect(&e?.path(), skip, files)?;
            }
        } else {
            private_file(path)?;
            // Failed temporary files are evidence, not reviewed input.
            if !path
                .file_name()
                .is_some_and(|s| s.to_string_lossy().starts_with(".live-catalog-"))
            {
                files.insert(path.to_string_lossy().into_owned(), digest(path)?);
            }
        }
        Ok(())
    }
    let config_path = root.join("config.json");
    let runtime = load(&config_path)?;
    require(
        runtime["reviewed_dir"] == "reviewed" && runtime["pack_dir"] == "packs",
        "live_catalog_runtime_layout",
    )?;
    let mut files = BTreeMap::new();
    let skip = root.join("reviewed/library/library.json");
    collect(&config_path, &skip, &mut files)?;
    for name in ["reviewed", "packs"] {
        collect(&root.join(name), &skip, &mut files)?;
    }
    for (key, value) in runtime.as_object().ok_or("live_catalog_runtime")? {
        if key.ends_with("_file") {
            let name = value.as_str().ok_or("live_catalog_runtime_file")?;
            require(
                Path::new(name).components().count() == 1 && name != "." && name != "..",
                "live_catalog_runtime_file",
            )?;
            collect(&root.join(name), &skip, &mut files)?;
        }
    }
    for key in ["token_file", "ca_file", "client_pem"] {
        if let Some(path) = c.get(key) {
            collect(
                Path::new(path.as_str().ok_or("live_catalog_probe_file")?),
                &skip,
                &mut files,
            )?;
        }
    }
    let binary = Path::new(text(&c["cli_binary"], "path")?);
    require(
        binary.is_absolute() && digest(binary)? == c["cli_binary"]["sha256"],
        "live_catalog_cli_pin",
    )?;
    files.insert(binary.to_string_lossy().into_owned(), digest(binary)?);
    Ok(json!(files))
}

fn probe(c: &Value, p: &Value, calls: &mut Calls) -> Result<Value> {
    let mut common = strings(&[
        "--json",
        "--auth",
        "external",
        "--transport",
        "production",
        "--gateway",
        text(c, "gateway")?,
        "--token-file",
        text(c, "token_file")?,
    ]);
    for (key, flag) in [
        ("ca_file", "--ca-file"),
        ("client_pem", "--client-pem"),
        ("connect_ip", "--connect-ip"),
        ("interface", "--interface"),
    ] {
        if let Some(value) = c.get(key) {
            common.extend(strings(&[
                flag,
                value.as_str().ok_or("live_catalog_probe_option")?,
            ]));
        }
    }
    let invoke =
        |calls: &mut Calls, label: &str, command: &[String], options: &[String]| -> Result<Value> {
            let mut argv = vec![text(&c["cli_binary"], "path")?.to_owned()];
            argv.extend_from_slice(command);
            argv.extend_from_slice(options);
            let response: Value = serde_json::from_slice(&calls.run(label, &argv, None)?)?;
            require(
                response.is_object() && response["error"].is_null(),
                "live_catalog_probe_error",
            )?;
            Ok(response)
        };
    let mut initial = common.clone();
    initial.extend(strings(&["--expected-publication-id", "current"]));
    let freshness = invoke(
        calls,
        "freshness",
        &strings(&["freshness", "--summary"]),
        &initial,
    )?;
    require(freshness["publication"] == "live", "live_catalog_freshness")?;
    let binding = &freshness["publication_binding"];
    let id = nonempty(binding, "publication_id")?;
    require(
        id.starts_with("pub1_") && id.len() == 69 && hex(&binding["data_vector_sha256"]),
        "live_catalog_probe_binding",
    )?;
    common.extend(strings(&[
        "--expected-publication-id",
        id,
        "--expected-data-vector-sha256",
        text(binding, "data_vector_sha256")?,
    ]));
    // The origin may be a proxy. Prove it serves THIS receipt, not merely equal rows.
    let full = invoke(
        calls,
        "freshness-receipt",
        &strings(&["freshness"]),
        &common,
    )?;
    let libraries = array(&full, "library_publications")?;
    let sources: BTreeSet<_> = array(&p["receipt"], "shards")?
        .iter()
        .map(|s| nonempty(s, "source_id"))
        .collect::<Result<_>>()?;
    let observed: BTreeSet<_> = libraries
        .iter()
        .map(|s| nonempty(s, "source_id"))
        .collect::<Result<_>>()?;
    require(
        full["publication_binding"] == *binding
            && full["publication"] == "live"
            && sources == observed
            && libraries.len() == sources.len()
            && libraries.iter().all(|l| {
                l["ready"] == true
                    && l["library_id"] == p["receipt"]["library"]["library_id"]
                    && l["receipt_sha256"] == p["receipt_sha256"]
            }),
        "live_catalog_probe_receipt",
    )?;
    let manifest = invoke(
        calls,
        "manifest",
        &strings(&["knowledge", "cat", "manifest.json"]),
        &common,
    )?;
    require(
        manifest["sha256"] == c["expected_manifest_sha256"]
            && manifest["publication_binding"] == *binding,
        "live_catalog_probe_manifest",
    )?;
    let mut checked = Vec::new();
    for (i, q) in array(c, "queries")?.iter().enumerate() {
        let sql = nonempty(q, "sql")?;
        let validation = invoke(
            calls,
            &format!("validate-{i}"),
            &strings(&["validate", sql]),
            &common,
        )?;
        require(
            validation["valid"] == true && validation["publication_binding"] == *binding,
            "live_catalog_probe_validate",
        )?;
        let query = invoke(
            calls,
            &format!("query-{i}"),
            &strings(&["query", sql]),
            &common,
        )?;
        require(
            query["truncated"] == false
                && query["rows"] == q["expected_rows"]
                && query["query_id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("q_"))
                && query["publication_binding"] == *binding,
            "live_catalog_probe_query",
        )?;
        checked
            .push(json!({"query_id":query["query_id"],"rows_sha256":sha(&bytes(&query["rows"])?)}));
    }
    Ok(json!({"publication_binding":binding,"freshness":freshness,
        "manifest_sha256":manifest["sha256"],"queries":checked}))
}

pub fn run(config_path: &Path, out: &Path, dry_run: bool) -> Result<Value> {
    let out = crate::refresh::output_path(out)?;
    dir(&out)?;
    let serial = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let attempt = out.join(format!("attempt-{serial}-{}", std::process::id()));
    fs::create_dir(&attempt)?;
    fs::set_permissions(&attempt, fs::Permissions::from_mode(0o700))?;
    let mut replaced = false;
    save(
        attempt.join("started.json"),
        &json!({"protocol":"oo.library.refresh-live-catalog-attempt/v1",
        "dry_run":dry_run,"config":config_path,"pid":std::process::id()}),
    )?;
    let result = execute(config_path, &attempt, dry_run, &mut replaced);
    let receipt = match &result {
        Ok(detail) => {
            json!({"success":true,"status":"succeeded","dry_run":dry_run,"catalog_replaced":replaced,
            "pack_digest":detail["binding"]["expected_pack_digest"],
            "schema_scope_digest":detail["binding"]["schema_scope_digest"],
            "attempt_directory":attempt,"detail":detail,"source_freshness":"published_source_snapshot_not_source_realtime"})
        }
        Err(e) => {
            json!({"success":false,"status":"failed","dry_run":dry_run,"catalog_replaced":replaced,
            "attempt_directory":attempt,"error":e.to_string(),"rollback_attempted":false})
        }
    };
    let mut receipt = receipt;
    if let Ok(detail) = &result {
        if !dry_run {
            receipt["publication_binding"] = detail["verification"]["publication_binding"].clone();
        }
    }
    save(attempt.join("result.json"), &receipt)?;
    File::open(&attempt)?.sync_all()?;
    result?;
    Ok(receipt)
}

fn execute(
    config_path: &Path,
    attempt: &Path,
    dry_run: bool,
    replaced: &mut bool,
) -> Result<Value> {
    private_file(config_path)?;
    let config_raw = fs::read(config_path)?;
    let c = parse(&config_raw)?;
    require(
        c["protocol"] == "oo.library.refresh-live-catalog/v1",
        "live_catalog_protocol",
    )?;
    let allowed = [
        "protocol",
        "gateway_root",
        "publication_receipt",
        "expected_pack_digest",
        "expected_manifest_sha256",
        "schema_scope_digest",
        "metadata_argv",
        "cli_binary",
        "gateway",
        "token_file",
        "ca_file",
        "client_pem",
        "connect_ip",
        "interface",
        "queries",
    ];
    require(
        c.as_object()
            .is_some_and(|o| o.keys().all(|k| allowed.contains(&k.as_str())))
            && [
                "expected_pack_digest",
                "expected_manifest_sha256",
                "schema_scope_digest",
            ]
            .iter()
            .all(|k| hex(&c[*k]))
            && hex(&c["cli_binary"]["sha256"]),
        "live_catalog_config",
    )?;
    let origin = nonempty(&c, "gateway")?;
    require(
        origin.starts_with("https://")
            && !origin[8..].is_empty()
            && !origin[8..].contains(['/', '@', '?', '#'])
            && !origin.chars().any(char::is_whitespace),
        "live_catalog_gateway_origin",
    )?;
    metadata_argv(&c)?;
    private_file(Path::new(text(&c, "token_file")?))?;
    require(
        !array(&c, "queries")?.is_empty(),
        "live_catalog_queries_required",
    )?;
    for q in array(&c, "queries")? {
        nonempty(q, "sql")?;
        require(q["expected_rows"].is_array(), "live_catalog_expected_rows")?;
    }
    let root = Path::new(text(&c, "gateway_root")?);
    directory(root)?;
    require(
        !attempt.starts_with(root) && !root.starts_with(attempt),
        "live_catalog_evidence_isolation",
    )?;
    let lock_path = root.join(".refresh-live-catalog.lock");
    if lock_path.exists() {
        private_file(&lock_path)?;
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)?;
    private_file(&lock_path)?;
    lock.try_lock().map_err(|_| "live_catalog_lock_busy")?;
    let lock_identity = (lock.metadata()?.dev(), lock.metadata()?.ino());
    let catalog_path = root.join("reviewed/library/library.json");
    let original = snapshot(&catalog_path)?;
    put(attempt.join("original-catalog.json"), &original.bytes)?;
    save(
        attempt.join("original.json"),
        &json!({"sha256":sha(&original.bytes),
        "identity":original.identity,"path":catalog_path}),
    )?;
    let catalog = parse(&original.bytes)?;
    for b in array(&catalog, "bindings")? {
        require(
            b.as_object().is_some_and(|o| o.len() == 6)
                && ["config_digest", "type_digest", "input_digest"]
                    .iter()
                    .all(|k| hex(&b[*k]))
                && b["generation"].as_i64().is_some_and(|n| n > 0),
            "live_catalog_original_six_fields",
        )?;
        nonempty(b, "dataset")?;
        nonempty(b, "source")?;
    }
    let receipt_path = Path::new(text(&c, "publication_receipt")?);
    private_file(receipt_path)?;
    let receipt_raw = fs::read(receipt_path)?;
    put(attempt.join("publication-input.json"), &receipt_raw)?;
    let published = parse(&receipt_raw)?;
    let p = &published["prepared"]["native_mapping"]["publication"];
    require(
        published["release"]["knowledge_published"] == true
            && published["release"]["pack_digest"] == c["expected_pack_digest"]
            && published["release"]["native_publication"]["receipt_sha256"] == p["receipt_sha256"],
        "live_catalog_release_required",
    )?;
    let scope = membership(p)?;
    require(
        scope == membership(&catalog["publication"])?
            && p["receipt"]["library"]["manifest"]
                == catalog["publication"]["receipt"]["library"]["manifest"]
            && scope == names(&catalog["visibility"], "datasets", "dataset")?
            && scope == names(&catalog["effective"], "datasets", "dataset")?
            && scope == names(&catalog, "bindings", "dataset")?
            && sha(&bytes(&json!(scope))?) == c["schema_scope_digest"],
        "live_catalog_exact_scope",
    )?;
    check_pack(&c, &catalog, root)?;
    let seal = runtime_seal(&c, root)?;
    let mut calls =
        Calls::with_operator(&attempt.join("calls"), &json!({"child_deadline_ms":120000}))?;
    let role = nonempty(&catalog["visibility"], "role")?;
    let before = metadata(&c, p, role, &mut calls, "metadata-before")?;
    let new_bindings = bindings(p, &before)?;
    let mut candidate = catalog.clone();
    candidate["publication"] = p.clone();
    candidate["bindings"] = new_bindings.clone();
    // Re-entry does not rewrite bytes, reorder bindings or change inode/mtime.
    let same = catalog["publication"] == *p;
    if same {
        let mut existing = array(&catalog, "bindings")?.clone();
        existing.sort_by(|a, b| a["dataset"].as_str().cmp(&b["dataset"].as_str()));
        require(
            json!(existing) == new_bindings,
            "live_catalog_same_receipt_binding_drift",
        )?;
    }
    let mut candidate_raw = bytes(&candidate)?;
    candidate_raw.push(b'\n');
    if same {
        candidate_raw = original.bytes.clone();
    }
    put(attempt.join("candidate-catalog.json"), &candidate_raw)?;
    let prepared = json!({"config_sha256":sha(&config_raw),"publication_file_sha256":sha(&receipt_raw),
        "publication_receipt_sha256":p["receipt_sha256"],"old_catalog_sha256":sha(&original.bytes),
        "candidate_catalog_sha256":sha(&candidate_raw),"same_receipt":same,
        "expected_pack_digest":c["expected_pack_digest"],"expected_manifest_sha256":c["expected_manifest_sha256"],
        "schema_scope_digest":c["schema_scope_digest"],"runtime_seal":seal});
    save(attempt.join("prepared.json"), &prepared)?;
    let temp = catalog_path
        .parent()
        .ok_or("live_catalog_parent")?
        .join(format!(
            ".live-catalog-{}.tmp",
            attempt.file_name().unwrap().to_string_lossy()
        ));
    if !dry_run && !same {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        f.write_all(&candidate_raw)?;
        f.sync_all()?;
    }
    let again = metadata(&c, p, role, &mut calls, "metadata-precommit")?;
    require(
        bindings(p, &again)? == new_bindings,
        "live_catalog_metadata_drift",
    )?;
    require(
        fs::read(config_path)? == config_raw
            && fs::read(receipt_path)? == receipt_raw
            && runtime_seal(&c, root)? == seal,
        "live_catalog_input_drift",
    )?;
    require(
        (
            fs::metadata(&lock_path)?.dev(),
            fs::metadata(&lock_path)?.ino(),
        ) == lock_identity,
        "live_catalog_lock_drift",
    )?;
    if !dry_run && !same {
        require(fs::read(&temp)? == candidate_raw, "live_catalog_temp_drift")?;
    }
    cas(&catalog_path, &original)?;
    if dry_run {
        return Ok(json!({"prepared":true,"activated":false,"verified":false,"binding":prepared}));
    }
    if !same {
        fs::rename(&temp, &catalog_path)?;
        *replaced = true;
        save(
            attempt.join("renamed.json"),
            &json!({"catalog_sha256":sha(&candidate_raw)}),
        )?;
        File::open(catalog_path.parent().unwrap())?.sync_all()?;
    }
    let activated = snapshot(&catalog_path)?;
    require(
        activated.bytes == candidate_raw,
        "live_catalog_post_rename_drift",
    )?;
    let verification = probe(&c, p, &mut calls)?;
    let final_metadata = metadata(&c, p, role, &mut calls, "metadata-after-probe")?;
    require(
        bindings(p, &final_metadata)? == new_bindings
            && runtime_seal(&c, root)? == seal
            && fs::read(config_path)? == config_raw
            && fs::read(receipt_path)? == receipt_raw,
        "live_catalog_post_probe_drift",
    )?;
    cas(&catalog_path, &activated)?;
    Ok(
        json!({"prepared":true,"activated":true,"verified":true,"same_receipt":same,
        "binding":prepared,"verification":verification}),
    )
}

#[cfg(test)]
#[path = "../tests/refresh_live_catalog/unit.rs"]
mod tests;
