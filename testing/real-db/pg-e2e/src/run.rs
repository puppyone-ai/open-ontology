//! CLI steps for the archived onboarding chain.

#![forbid(unsafe_code)]

use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::gold::{self, ACCOUNT_ROWS, ENTRY_ROWS, PARTY_ROWS, SOURCE_GENERATION};
use crate::json::{self, Value};
use crate::sha::sha256_hex;
use crate::Fail;

const BINS: &[&str] = &[
    "init-pg",
    "oo-pipeline",
    "oo-builder",
    "reviewed-effective-oos",
    "reviewed-policy",
    "install-column-acl",
    "provision-mapping-reader",
    "analyze-pg",
    "role-pack-bundle",
    "query-pg",
];

const DECISIONS: &[(&str, &str, &str, &str)] = &[
    ("shop.party", "party_key", "none", "identifier"),
    ("shop.party", "display_name", "personal", "person_name"),
    ("shop.party", "active", "none", "boolean_flag"),
    ("shop.party", "phone", "personal", "free_text"),
    ("shop.account", "account_id", "none", "identifier"),
    ("shop.account", "party_key", "none", "foreign_key"),
    ("shop.account", "opened", "none", "boolean_flag"),
    ("shop.account", "label", "none", "free_text"),
    ("shop.entry", "entry_id", "none", "identifier"),
    ("shop.entry", "account_id", "none", "foreign_key"),
    ("shop.entry", "qty", "none", "quantity"),
    ("shop.entry", "amount", "none", "quantity"),
    ("shop.entry", "memo", "none", "free_text"),
];

pub fn emit(dir: &Path) -> Result<(), Fail> {
    fs::create_dir_all(dir).map_err(|_| crate::fail("emit dir"))?;
    let mut party = File::create(dir.join("party.copy")).map_err(|_| crate::fail("emit file"))?;
    gold::write_party(&mut party)?;
    let mut account = File::create(dir.join("account.copy")).map_err(|_| crate::fail("emit file"))?;
    gold::write_account(&mut account)?;
    let mut entry = File::create(dir.join("entry.copy")).map_err(|_| crate::fail("emit file"))?;
    gold::write_entry(&mut entry)?;
    let note = format!(
        "party={PARTY_ROWS}\naccount={ACCOUNT_ROWS}\nentry={ENTRY_ROWS}\ngeneration={SOURCE_GENERATION}\n"
    );
    fs::write(dir.join("counts.txt"), note).map_err(|_| crate::fail("emit counts"))?;
    Ok(())
}

pub fn inspect_config(bins: &Path, secrets: &Path, ready: &Path) -> Result<String, Fail> {
    let init = pin(bins, "init-pg")?;
    let pipeline = pin(bins, "oo-pipeline")?;
    let builder = pin(bins, "oo-builder")?;
    Ok(format!(
        "{{\"protocol\":\"oo.onboarding.inspect/v1\",\"caller\":\"trusted_local_admin\",\"binaries\":{{\"init_pg\":{init},\"import_pg\":{pipeline},\"catalog_pg\":{pipeline},\"captured_pg_draft\":{builder}}},\"source\":{{\"source_id\":\"pubfix\",\"host\":\"oo-a317-pg\",\"port\":5432,\"database\":\"oo_a317_src\",\"username\":\"oo_a317_reader\",\"secret_ref\":\"secret://pg/a317/reader\",\"password_file\":{reader},\"ca_file\":{ca},\"tables\":[{{\"schema\":\"shop\",\"table\":\"party\"}},{{\"schema\":\"shop\",\"table\":\"account\"}},{{\"schema\":\"shop\",\"table\":\"entry\"}}]}},\"target\":{{\"host\":\"oo-a317-pg\",\"port\":5432,\"database\":\"oo_a317_dst\",\"administrator\":\"oo_fixture_admin\",\"runtime_principal\":\"oo_a317_runtime\",\"admin_password_file\":{admin},\"runtime_password_file\":{runtime},\"ca_file\":{ca}}},\"shard\":\"a317main\",\"owner\":\"eef831aa-dba4-4af2-a78b-541bc9e0a317\",\"source_generation\":317,\"connect_ms\":30000,\"runtime_membership_ready_file\":{ready},\"membership_wait_ms\":180000,\"comment_markers\":[\"甲注评\"],\"deadlines_ms\":{{\"import_ms\":180000}}}}\n",
        reader = json::escape(&path_text(&secrets.join("reader.pass"))?),
        ca = json::escape(&path_text(&secrets.join("ca.crt"))?),
        admin = json::escape(&path_text(&secrets.join("admin.pass"))?),
        runtime = json::escape(&path_text(&secrets.join("runtime.pass"))?),
        ready = json::escape(&path_text(ready)?),
    ))
}

pub fn bind(captured_path: &Path, work: &Path) -> Result<(), Fail> {
    let captured = json::parse_file(captured_path)?;
    let schema_hash = captured
        .get("wrapper")
        .and_then(|value| value.get("schema_hash"))
        .and_then(Value::as_str)
        .ok_or_else(|| crate::fail("schema_hash"))?;
    if schema_hash.len() != 64 {
        return Err(crate::fail("schema_hash"));
    }
    let queue = captured
        .get("draft")
        .and_then(|value| value.get("review_queue"))
        .and_then(Value::as_array)
        .ok_or_else(|| crate::fail("review_queue"))?;
    let mut reviews = Vec::new();
    for item in queue {
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("");
        let item_id = item.get("item_id").and_then(Value::as_str).unwrap_or("");
        let structural = matches!(
            kind,
            "structural_column" | "structural_primary_key" | "candidate_relationship"
        );
        let skipped = item_id.starts_with("pii/")
            || item_id.starts_with("semantic/")
            || item_id.starts_with("type/")
            || item_id.starts_with("classification/");
        if !structural || skipped {
            continue;
        }
        let fingerprint = item
            .get("fact_fingerprint")
            .and_then(Value::as_str)
            .ok_or_else(|| crate::fail("fact_fingerprint"))?;
        reviews.push(format!(
            "{{\"item_id\":{},\"action\":\"accept\",\"schema_hash\":{},\"fact_fingerprint\":{}}}",
            json::escape(item_id),
            json::escape(schema_hash),
            json::escape(fingerprint)
        ));
    }
    if reviews.is_empty() {
        return Err(crate::fail("reviews empty"));
    }
    let mut decisions = Vec::new();
    for (dataset, column, pii, semantic) in DECISIONS {
        let item_id = format!("column/{dataset}/{column}");
        let fingerprint = queue
            .iter()
            .find(|item| {
                item.get("item_id").and_then(Value::as_str) == Some(item_id.as_str())
                    && item.get("kind").and_then(Value::as_str) == Some("structural_column")
            })
            .and_then(|item| item.get("fact_fingerprint"))
            .and_then(Value::as_str)
            .ok_or_else(|| crate::fail(format!("missing fingerprint {item_id}")))?;
        decisions.push(format!(
            "{{\"dataset\":{},\"column\":{},\"schema_hash\":{},\"fact_fingerprint\":{},\"pii\":{},\"semantic_type\":{},\"reviewer\":{{\"by\":\"public-fixture\",\"at\":\"2026-10-07\"}}}}",
            json::escape(dataset),
            json::escape(column),
            json::escape(schema_hash),
            json::escape(fingerprint),
            json::escape(pii),
            json::escape(semantic)
        ));
    }
    fs::create_dir_all(work).map_err(|_| crate::fail("work dir"))?;
    fs::write(work.join("reviews.json"), format!("[{}]\n", reviews.join(",")))
        .map_err(|_| crate::fail("reviews write"))?;
    fs::write(work.join("decisions.json"), format!("[{}]\n", decisions.join(",")))
        .map_err(|_| crate::fail("decisions write"))?;
    fs::write(work.join("vis-wide.json"), visibility("oo_r_wide", true))
        .map_err(|_| crate::fail("visibility write"))?;
    fs::write(work.join("vis-narrow.json"), visibility("oo_r_narrow", false))
        .map_err(|_| crate::fail("visibility write"))?;
    Ok(())
}

fn visibility(role: &str, wide: bool) -> String {
    let party = if wide {
        "[\"party_key\",\"display_name\",\"active\",\"phone\"]"
    } else {
        "[\"party_key\",\"display_name\"]"
    };
    let account = if wide {
        "[\"account_id\",\"party_key\",\"opened\",\"label\"]"
    } else {
        "[\"account_id\",\"party_key\",\"opened\"]"
    };
    let entry = if wide {
        "[\"entry_id\",\"account_id\",\"qty\",\"amount\",\"memo\"]"
    } else {
        "[\"entry_id\",\"account_id\"]"
    };
    format!(
        "{{\"protocol\":\"oo.builder.role-visibility/v1\",\"project\":\"pubfix\",\"principal\":\"fixture_operator\",\"role\":{role},\"ontology_version\":\"ont-a317\",\"catalog_version\":\"cat-a317\",\"policy\":{{\"id\":\"pol-a317\",\"version\":\"1\"}},\"knowledge\":{{\"id\":\"know-a317\",\"version\":\"1\"}},\"issued_for_compiler\":\"oo-builder/role-pack/v1\",\"datasets\":[{{\"dataset\":\"shop.party\",\"row_security\":\"none\",\"global_stats\":\"denied\",\"columns\":{party}}},{{\"dataset\":\"shop.account\",\"row_security\":\"none\",\"global_stats\":\"denied\",\"columns\":{account}}},{{\"dataset\":\"shop.entry\",\"row_security\":\"none\",\"global_stats\":\"denied\",\"columns\":{entry}}}],\"limits\":{{\"max_file_bytes\":262144,\"max_total_bytes\":1048576,\"max_entries\":128}}}}\n",
        role = json::escape(role)
    )
}

pub fn apply_config(bins: &Path, secrets: &Path, work: &Path) -> Result<String, Fail> {
    let builder = pin(bins, "oo-builder")?;
    let effective = pin(bins, "reviewed-effective-oos")?;
    let policy = pin(bins, "reviewed-policy")?;
    let install = pin(bins, "install-column-acl")?;
    let provision = pin(bins, "provision-mapping-reader")?;
    let analyze = pin(bins, "analyze-pg")?;
    let bundle = pin(bins, "role-pack-bundle")?;
    let query = pin(bins, "query-pg")?;
    let reviews = json::escape(&path_text(&work.join("reviews.json"))?);
    let decisions = json::escape(&path_text(&work.join("decisions.json"))?);
    let wide_vis = json::escape(&path_text(&work.join("vis-wide.json"))?);
    let narrow_vis = json::escape(&path_text(&work.join("vis-narrow.json"))?);
    let login = json::escape(&path_text(&secrets.join("login.pass"))?);
    Ok(format!(
        "{{\"protocol\":\"oo.onboarding.apply-reviewed/v1\",\"caller\":\"trusted_local_admin\",\"binaries\":{{\"review_pg_draft\":{builder},\"reviewed_effective_oos\":{effective},\"reviewed_policy\":{policy},\"install_column_acl\":{install},\"provision_mapping_reader\":{provision},\"analyze_pg\":{analyze},\"role_pack\":{builder},\"role_pack_bundle\":{bundle},\"query_pg\":{query}}},\"reviews_file\":{reviews},\"decisions_file\":{decisions},\"settings\":{settings},\"schema\":\"shop\",\"query_login\":{{\"user\":\"oo_a317_login\",\"password_file\":{login}}},\"roles\":[{wide},{narrow}],\"analyze\":{{\"statement_timeout_ms\":30000,\"lock_timeout_ms\":30000,\"connect_ms\":30000}}}}\n",
        settings = settings_json(),
        wide = role_json("wide", true, &wide_vis),
        narrow = role_json("narrow", false, &narrow_vis),
    ))
}

fn settings_json() -> &'static str {
    "{\"project\":\"pubfix\",\"ontology_version\":\"ont-a317\",\"catalog_version\":\"cat-a317\",\"policy\":{\"id\":\"pol-a317\",\"version\":\"1\"},\"knowledge\":{\"id\":\"know-a317\",\"version\":\"1\"},\"ontology\":{\"spec_version\":\"oos/v1\",\"name\":\"shop\",\"locale\":\"en-US\",\"include_schemas\":[\"shop\"],\"default_tier_preference\":[\"raw\"],\"publish\":{\"include_unreviewed\":[]},\"classification\":{\"auto_accept\":\"rule_high_confidence\"},\"lifecycle\":{\"deprecation_min_releases\":2},\"compile\":{\"per_role\":true,\"budgets\":{\"readme_tokens\":2000,\"table_file_tokens\":3000},\"sample_values\":{\"default\":\"masked\",\"max_per_column\":0}}}}"
}

fn role_json(slug: &str, wide: bool, visibility: &str) -> String {
    format!(
        "{{\"slug\":{slug},\"allow\":{allow},\"visibility_file\":{visibility},\"queries\":{queries}}}",
        slug = json::escape(slug),
        allow = allow_json(wide),
        queries = if wide { wide_queries() } else { narrow_queries() }
    )
}

fn allow_json(wide: bool) -> String {
    let mut items = Vec::new();
    for (dataset, column, pii, _) in DECISIONS {
        if !wide && !narrow_sees(dataset, column) {
            continue;
        }
        if *dataset == "shop.party" && *column == "national_id" {
            continue;
        }
        let explicit = *pii == "personal" || *pii == "sensitive";
        items.push(format!(
            "{{\"dataset\":{},\"column\":{},\"explicit_allow\":{}}}",
            json::escape(dataset),
            json::escape(column),
            if explicit { "true" } else { "false" }
        ));
    }
    format!("[{}]", items.join(","))
}

fn narrow_sees(dataset: &str, column: &str) -> bool {
    matches!(
        (dataset, column),
        ("shop.party", "party_key")
            | ("shop.party", "display_name")
            | ("shop.account", "account_id")
            | ("shop.account", "party_key")
            | ("shop.account", "opened")
            | ("shop.entry", "entry_id")
            | ("shop.entry", "account_id")
    )
}

fn statement(sql: &str, max_rows: u32) -> String {
    format!(
        "{{\"source\":\"request\",\"sql\":{},\"max_rows\":{max_rows},\"max_result_bytes\":8192,\"wall_clock_ms\":30000}}",
        json::escape(sql)
    )
}

fn query(name: &str, when: &str, exit: i32, stderr: Option<&str>, sql: &str, max_rows: u32) -> String {
    let stderr_json = match stderr {
        Some(text) => json::escape(text),
        None => "null".to_owned(),
    };
    format!(
        "{{\"name\":{},\"when\":{},\"expected_exit\":{exit},\"expected_stderr\":{stderr_json},\"statement\":{}}}",
        json::escape(name),
        json::escape(when),
        statement(sql, max_rows)
    )
}

fn wide_queries() -> String {
    format!(
        "[{},{},{},{},{},{},{},{},{}]",
        query("wide-before", "before_analyze", 5, Some("query-pg: policy"), "SELECT party_key FROM shop.party ORDER BY party_key", 20),
        query("wide-hidden", "after_bundle", 6, Some("query-pg: guard"), "SELECT national_id FROM shop.party", 20),
        query("wide-insert", "after_bundle", 6, Some("query-pg: guard"), "INSERT INTO shop.party (party_key, display_name, active, phone, national_id) VALUES ('p99999', 'x', true, 'tel-x', 'nid-x')", 20),
        query("wide-pos", "after_bundle", 6, Some("query-pg: guard"), "SELECT party_key FROM shop.party ORDER BY _oo_pos", 20),
        query("wide-count", "after_bundle", 0, None, "SELECT count(*) FROM shop.entry", 1),
        query("wide-null", "after_bundle", 0, None, "SELECT party_key, display_name, active FROM shop.party WHERE party_key = 'p00000'", 1),
        query("wide-qty", "after_bundle", 0, None, "SELECT entry_id, qty FROM shop.entry WHERE entry_id IN (0, 1, 2) ORDER BY entry_id", 3),
        query("wide-fk", "after_bundle", 0, None, "SELECT account_id, party_key FROM shop.account WHERE account_id = 0", 1),
        query("wide-phone", "after_bundle", 0, None, "SELECT phone FROM shop.party WHERE party_key = 'p00001'", 1),
    )
}

fn narrow_queries() -> String {
    format!(
        "[{},{},{},{}]",
        query("narrow-party", "after_bundle", 0, None, "SELECT party_key, display_name FROM shop.party WHERE party_key = 'p00001'", 1),
        query("narrow-hidden", "after_bundle", 6, Some("query-pg: guard"), "SELECT national_id FROM shop.party", 20),
        query("narrow-phone", "after_bundle", 6, Some("query-pg: guard"), "SELECT phone FROM shop.party", 20),
        query("narrow-amount", "after_bundle", 6, Some("query-pg: guard"), "SELECT amount FROM shop.entry", 20),
    )
}

pub fn check_inspect(run: &Path) -> Result<(), Fail> {
    let outcome = json::parse_file(&run.join("outcome.json"))?;
    expect_str(&outcome, "status", "awaiting_review")?;
    expect_i64(&outcome, "source_cut", SOURCE_GENERATION)?;
    expect_i64(&outcome, "mapping_run_generation", SOURCE_GENERATION)?;
    let fence = outcome
        .get("target_fence")
        .and_then(Value::as_i64)
        .ok_or_else(|| crate::fail("target_fence"))?;
    if fence == SOURCE_GENERATION {
        return Err(crate::fail("target_fence collided with source_cut"));
    }
    let manifest = json::parse_file(&run.join("inspect-manifest.json"))?;
    expect_str(&manifest, "config_identity_sha256", &gold::config_identity())?;
    expect_i64(&manifest, "source_cut", SOURCE_GENERATION)?;
    absent_marker(&run.join("catalog.json"))?;
    absent_marker(&run.join("captured-draft.json"))?;
    let claims = outcome.get("non_claims").ok_or_else(|| crate::fail("non_claims"))?;
    expect_non_claims(claims)?;
    let import = json::parse_file(&run.join("import-result.json"))?;
    expect_i64(&import, "source_generation", SOURCE_GENERATION)?;
    expect_rows(&import, "party", u64::from(PARTY_ROWS))?;
    expect_rows(&import, "account", u64::from(ACCOUNT_ROWS))?;
    expect_rows(&import, "entry", u64::from(ENTRY_ROWS))?;
    Ok(())
}

pub fn check_apply(run: &Path, secrets: &Path) -> Result<(), Fail> {
    let applied = json::parse_file(&run.join("apply-outcome.json"))?;
    expect_str(&applied, "status", "ready_for_trusted_query")?;
    expect_i64(&applied, "source_cut", SOURCE_GENERATION)?;
    expect_i64(&applied, "mapping_run_generation", SOURCE_GENERATION)?;
    let fence = applied
        .get("target_fence")
        .and_then(Value::as_i64)
        .ok_or_else(|| crate::fail("target_fence"))?;
    if fence == SOURCE_GENERATION {
        return Err(crate::fail("target_fence collided with source_cut"));
    }
    expect_bool(&applied, "analyze_published", false)?;
    expect_bool(&applied, "estimate_is_exact", false)?;
    expect_bool(&applied, "database_rollback", false)?;
    expect_bool(&applied, "database_release", false)?;
    expect_str(&applied, "database_state", "child_compilers_applied_no_release")?;
    let claims = applied.get("non_claims").ok_or_else(|| crate::fail("non_claims"))?;
    expect_non_claims(claims)?;
    let hidden = applied
        .get("hidden")
        .and_then(Value::as_array)
        .ok_or_else(|| crate::fail("hidden"))?;
    let note = hidden
        .iter()
        .find(|item| {
            item.get("dataset").and_then(Value::as_str) == Some("shop.party")
                && item.get("column").and_then(Value::as_str) == Some("national_id")
        })
        .ok_or_else(|| crate::fail("hidden national_id"))?;
    expect_str(note, "reason", "classification.absent")?;
    let provision = applied
        .get("provision")
        .and_then(Value::as_array)
        .ok_or_else(|| crate::fail("provision"))?;
    if provision.len() != 2 {
        return Err(crate::fail("provision width"));
    }
    expect_str(&provision[0], "effect", "installed")?;
    expect_str(&provision[1], "effect", "replay")?;
    for slug in ["wide", "narrow"] {
        let value = json::parse_file(&run.join("steps").join(format!("install-column-acl-{slug}.stdout")))?;
        expect_str(&value, "effect", "installed")?;
        expect_str(&value, "grantor", "oo_fixture_admin")?;
        expect_str(&value, "role", &format!("oo_r_{slug}"))?;
    }
    check_rows(
        run,
        "wide-count",
        &[vec![Cell::Int("90000")]],
    )?;
    check_rows(
        run,
        "wide-null",
        &[vec![Cell::Text("p00000"), Cell::Null, Cell::Bool(true)]],
    )?;
    check_rows(
        run,
        "wide-qty",
        &[
            vec![Cell::Int("0"), Cell::Int("-9223372036854775808")],
            vec![Cell::Int("1"), Cell::Int("9223372036854775807")],
            vec![Cell::Int("2"), Cell::Int("0")],
        ],
    )?;
    check_rows(
        run,
        "wide-fk",
        &[vec![Cell::Int("0"), Cell::Text("p00000")]],
    )?;
    check_rows(run, "wide-phone", &[vec![Cell::Text("tel-00001")]])?;
    check_rows(
        run,
        "narrow-party",
        &[vec![Cell::Text("p00001"), Cell::Text("n00001")]],
    )?;
    for name in [
        "wide-before",
        "wide-hidden",
        "wide-insert",
        "wide-pos",
        "narrow-hidden",
        "narrow-phone",
        "narrow-amount",
    ] {
        let stdout = fs::read(run.join("steps").join(format!("query-{name}.stdout")))
            .map_err(|_| crate::fail(format!("missing {name}")))?;
        if !stdout.is_empty() {
            return Err(crate::fail(format!("{name} wrote stdout")));
        }
        if run.join("audit").join(format!("{name}.jsonl")).exists() {
            return Err(crate::fail(format!("{name} wrote an audit")));
        }
    }
    let digest = gold::config_identity();
    let passwords = passwords(secrets)?;
    for (name, role, sql) in [
        ("wide-count", "oo_r_wide", "SELECT count(*) FROM shop.entry"),
        ("wide-null", "oo_r_wide", "SELECT party_key, display_name, active FROM shop.party WHERE party_key = 'p00000'"),
        ("wide-qty", "oo_r_wide", "SELECT entry_id, qty FROM shop.entry WHERE entry_id IN (0, 1, 2) ORDER BY entry_id"),
        ("wide-fk", "oo_r_wide", "SELECT account_id, party_key FROM shop.account WHERE account_id = 0"),
        ("wide-phone", "oo_r_wide", "SELECT phone FROM shop.party WHERE party_key = 'p00001'"),
        ("narrow-party", "oo_r_narrow", "SELECT party_key, display_name FROM shop.party WHERE party_key = 'p00001'"),
    ] {
        check_audit(run, name, role, &digest, sql, &passwords)?;
    }
    for name in ["inspect-manifest.json", "outcome.json", "apply-outcome.json", "catalog.json"] {
        let bytes = fs::read(run.join(name)).map_err(|_| crate::fail(format!("read {name}")))?;
        for secret in &passwords {
            if !secret.is_empty() && bytes.windows(secret.len()).any(|window| window == secret.as_slice()) {
                return Err(crate::fail(format!("{name} contains a password")));
            }
        }
    }
    Ok(())
}

pub fn copy_sql(import_path: &Path, table: &str) -> Result<String, Fail> {
    let import = json::parse_file(import_path)?;
    let (physical, columns) = table_physical(&import, table)?;
    let order = match table {
        "party" => ["party_key", "display_name", "active", "phone", "national_id"],
        "account" => ["account_id", "party_key", "opened", "label", ""],
        "entry" => ["entry_id", "account_id", "qty", "amount", "memo"],
        _ => return Err(crate::fail("copy table")),
    };
    let wanted: Vec<&str> = order.into_iter().filter(|name| !name.is_empty()).collect();
    let mut select = Vec::new();
    for name in &wanted {
        let physical_column = columns
            .iter()
            .find(|(column, _)| column == name)
            .map(|(_, physical)| physical.clone())
            .ok_or_else(|| crate::fail(format!("missing column {name}")))?;
        select.push(quote_ident(&physical_column)?);
    }
    let key = quote_ident(
        columns
            .iter()
            .find(|(column, _)| *column == wanted[0])
            .map(|(_, physical)| physical.as_str())
            .ok_or_else(|| crate::fail("missing key"))?,
    )?;
    Ok(format!(
        "COPY (SELECT {} FROM _oo.{} ORDER BY {}) TO STDOUT\n",
        select.join(", "),
        quote_ident(&physical)?,
        key
    ))
}

pub fn check_copy(table: &str, path: &Path) -> Result<(), Fail> {
    let expected = gold::row_count(table)?;
    let file = File::open(path).map_err(|_| crate::fail(format!("open {}", path.display())))?;
    let mut lines = BufReader::new(file).lines();
    for index in 0..expected {
        let got = lines
            .next()
            .ok_or_else(|| crate::fail(format!("{table} ended at {index}")))?
            .map_err(|_| crate::fail("copy read"))?;
        let mut wanted = gold::line_at(table, index)?;
        if wanted.ends_with('\n') {
            wanted.pop();
        }
        if got != wanted {
            return Err(crate::fail(format!("{table} line {index} mismatch")));
        }
    }
    if lines.next().is_some() {
        return Err(crate::fail(format!("{table} has extra rows")));
    }
    Ok(())
}

pub fn query_request(
    manifest_path: &Path,
    policy_path: &Path,
    login_password: &Path,
    audit: &Path,
    sql: &str,
    max_rows: u32,
) -> Result<String, Fail> {
    let manifest = json::parse_file(manifest_path)?;
    let policy_doc = json::parse_file(policy_path)?;
    let policies = policy_doc
        .get("policies")
        .and_then(Value::as_array)
        .ok_or_else(|| crate::fail("policies"))?;
    let stripped: Vec<Value> = policies.iter().map(json::without_null_row_filter).collect();
    let rendered = json::render(&Value::Array(stripped));
    let target = manifest.get("target").ok_or_else(|| crate::fail("target"))?;
    let host = json::string_field(target, "host")?;
    let port = target
        .get("port")
        .and_then(Value::as_i64)
        .ok_or_else(|| crate::fail("port"))?;
    let database = json::string_field(target, "database")?;
    let ca = json::string_field(target, "ca_file")?;
    let digest = gold::config_identity();
    Ok(format!(
        "{{\"version\":\"oo.gateway.query-pg/v1\",\"endpoint\":{{\"host\":{},\"port\":{port},\"database\":{},\"user\":\"oo_a317_login\"}},\"password_file\":{},\"ca_file\":{},\"audit_file\":{},\"identity\":{{\"project\":\"pubfix\",\"role\":\"oo_r_wide\",\"policy_id\":{}}},\"policies\":{rendered},\"catalog\":{{\"max_tables\":4,\"max_columns\":64,\"max_functions\":1,\"max_operators\":64,\"large_table_row_threshold\":1000000,\"unknown_rows\":\"refuse\"}},\"statement\":{{\"source\":\"request\",\"sql\":{},\"max_rows\":{max_rows},\"max_result_bytes\":8192,\"wall_clock_ms\":30000}}}}\n",
        json::escape(host),
        json::escape(database),
        json::escape(&path_text(login_password)?),
        json::escape(ca),
        json::escape(&path_text(audit)?),
        json::escape(&digest),
        json::escape(sql),
    ))
}

pub fn fresh_request(manifest: &Path, policy: &Path, login_password: &Path, audit: &Path) -> Result<String, Fail> {
    query_request(
        manifest,
        policy,
        login_password,
        audit,
        "SELECT count(*) FROM shop.party",
        1,
    )
}

pub fn probe_request(manifest: &Path, policy: &Path, login_password: &Path, audit: &Path) -> Result<String, Fail> {
    query_request(
        manifest,
        policy,
        login_password,
        audit,
        "SELECT amount FROM shop.entry WHERE entry_id = 3",
        1,
    )
}

pub fn check_fresh(stdout_path: &Path, audit_path: &Path, secrets: &Path) -> Result<(), Fail> {
    let stdout = fs::read(stdout_path).map_err(|_| crate::fail("fresh stdout"))?;
    let lines = json::lines(&stdout)?;
    let rows: Vec<&Value> = lines
        .iter()
        .filter(|line| line.get("record").and_then(Value::as_str) == Some("row"))
        .collect();
    if rows.len() != 1 {
        return Err(crate::fail("fresh row count"));
    }
    expect_cell(rows[0], &[Cell::Int("2000")])?;
    let summary = lines
        .iter()
        .find(|line| line.get("record").and_then(Value::as_str) == Some("summary"))
        .ok_or_else(|| crate::fail("fresh summary"))?;
    expect_str(summary, "status", "succeeded")?;
    expect_bool(summary, "truncated", false)?;
    let passwords = passwords(secrets)?;
    check_audit_file(
        audit_path,
        "oo_r_wide",
        &gold::config_identity(),
        "SELECT count(*) FROM shop.party",
        &passwords,
    )?;
    Ok(())
}

pub fn note_probe(exit_path: &Path, stdout_path: &Path) -> Result<String, Fail> {
    let exit_text = fs::read_to_string(exit_path).map_err(|_| crate::fail("probe exit"))?;
    let exit_code = exit_text.trim();
    let stdout = fs::read(stdout_path).unwrap_or_default();
    let kind = if stdout.is_empty() {
        "empty-stdout"
    } else if let Ok(lines) = json::lines(&stdout) {
        if lines.iter().any(|line| line.get("record").and_then(Value::as_str) == Some("row")) {
            "row"
        } else {
            "no-row"
        }
    } else {
        "unparsed-stdout"
    };
    Ok(format!("probe_exit={exit_code}\nprobe_stdout={kind}\nnumeric_cell_set=null_bool_int_text\n"))
}

pub fn record_measurements(import_path: &Path, manifest_path: &Path, out: &Path) -> Result<(), Fail> {
    let import = json::parse_file(import_path)?;
    let manifest = json::parse_file(manifest_path)?;
    let budgets = manifest.get("budget_defaults").ok_or_else(|| crate::fail("budget_defaults"))?;
    let tables = import.get("tables").and_then(Value::as_array).ok_or_else(|| crate::fail("tables"))?;
    let mut text = String::new();
    for table in ["party", "account", "entry"] {
        let found = tables
            .iter()
            .find(|item| item.get("name").and_then(Value::as_str) == Some(table))
            .ok_or_else(|| crate::fail(format!("missing {table}")))?;
        let rows = found.get("rows").and_then(Value::as_i64).ok_or_else(|| crate::fail("rows"))?;
        let digest = found
            .get("canonical_digest")
            .and_then(Value::as_str)
            .ok_or_else(|| crate::fail("canonical_digest"))?;
        if digest.len() != 64 || !hex64(digest) {
            return Err(crate::fail("canonical_digest"));
        }
        text.push_str(&format!("{table}_rows={rows}\n{table}_canonical_digest={digest}\n"));
    }
    for key in ["connect_ms", "statement_ms", "lock_ms", "max_rows", "max_bytes"] {
        let got = budgets.get(key).and_then(Value::as_i64).ok_or_else(|| crate::fail(format!("budget {key}")))?;
        text.push_str(&format!("budget_{key}={got}\n"));
    }
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).map_err(|_| crate::fail("measurements dir"))?;
    }
    fs::write(out, text).map_err(|_| crate::fail("measurements"))
}

pub fn pack_digest(path: &Path) -> Result<String, Fail> {
    let value = json::parse_file(path)?;
    let digest = json::string_field(&value, "pack_digest")?;
    if digest.len() != 64 || !hex64(digest) {
        return Err(crate::fail("pack_digest"));
    }
    Ok(digest.to_owned())
}

fn expect_non_claims(claims: &Value) -> Result<(), Fail> {
    expect_bool(claims, "parent_acceptance", false)?;
    expect_bool(claims, "development_unit_approval", false)?;
    expect_bool(claims, "component_approval", false)?;
    expect_str(claims, "m1_i01", "not_accepted")?;
    expect_str(claims, "m1_i02", "not_accepted")?;
    expect_str(claims, "semantic_publication", "missing")?;
    expect_str(claims, "authentication", "not_claimed")?;
    expect_str(claims, "authorization", "not_claimed")?;
    expect_str(claims, "natural_language", "missing")?;
    expect_str(claims, "harness_mount", "not_performed")?;
    expect_bool(claims, "analyze_published", false)?;
    expect_bool(claims, "mapping_rows_read", false)?;
    expect_bool(claims, "public_auth", false)?;
    expect_str(claims, "config_identity", "recomputed_from_non_secret_request_fields")
}

fn hex64(text: &str) -> bool {
    text.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn table_physical(import: &Value, table: &str) -> Result<(String, Vec<(String, String)>), Fail> {
    let tables = import
        .get("tables")
        .and_then(Value::as_array)
        .ok_or_else(|| crate::fail("tables"))?;
    let found = tables
        .iter()
        .find(|item| {
            item.get("schema").and_then(Value::as_str) == Some("shop")
                && item.get("name").and_then(Value::as_str) == Some(table)
        })
        .ok_or_else(|| crate::fail("table missing"))?;
    let physical = found
        .get("physical_name")
        .and_then(Value::as_str)
        .ok_or_else(|| crate::fail("physical table"))?
        .to_owned();
    if !physical_name(&physical, "pt_") {
        return Err(crate::fail("physical table"));
    }
    let columns = found
        .get("columns")
        .and_then(Value::as_array)
        .ok_or_else(|| crate::fail("columns"))?;
    let mut pairs = Vec::new();
    for column in columns {
        let name = column
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| crate::fail("column name"))?
            .to_owned();
        let physical_column = column
            .get("physical_name")
            .and_then(Value::as_str)
            .ok_or_else(|| crate::fail("physical column"))?
            .to_owned();
        if !physical_name(&physical_column, "pc_") {
            return Err(crate::fail("physical column"));
        }
        pairs.push((name, physical_column));
    }
    Ok((physical, pairs))
}

fn physical_name(name: &str, prefix: &str) -> bool {
    let Some(rest) = name.strip_prefix(prefix) else {
        return false;
    };
    rest.len() == 60 && rest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn quote_ident(name: &str) -> Result<String, Fail> {
    if name.contains('"') || name.contains('\0') {
        return Err(crate::fail("ident"));
    }
    Ok(format!("\"{name}\""))
}

fn expect_rows(import: &Value, table: &str, rows: u64) -> Result<(), Fail> {
    let tables = import
        .get("tables")
        .and_then(Value::as_array)
        .ok_or_else(|| crate::fail("tables"))?;
    let found = tables
        .iter()
        .find(|item| item.get("name").and_then(Value::as_str) == Some(table))
        .ok_or_else(|| crate::fail(format!("missing {table}")))?;
    let got = found.get("rows").and_then(Value::as_i64).ok_or_else(|| crate::fail("rows"))?;
    if got != i64::try_from(rows).unwrap_or(-1) {
        return Err(crate::fail(format!("{table} rows {got}")));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Cell {
    Null,
    Bool(bool),
    Int(&'static str),
    Text(&'static str),
}

fn check_rows(run: &Path, name: &str, expect: &[Vec<Cell>]) -> Result<(), Fail> {
    let stdout = fs::read(run.join("steps").join(format!("query-{name}.stdout")))
        .map_err(|_| crate::fail(format!("missing {name}")))?;
    let lines = json::lines(&stdout)?;
    let rows: Vec<&Value> = lines
        .iter()
        .filter(|line| line.get("record").and_then(Value::as_str) == Some("row") && line.get("turn").and_then(Value::as_i64) == Some(0))
        .collect();
    if rows.len() != expect.len() {
        return Err(crate::fail(format!("{name} row count {}", rows.len())));
    }
    for (index, wanted) in expect.iter().enumerate() {
        expect_cell(rows[index], wanted).map_err(|err| crate::fail(format!("{name} row {index}: {err}")))?;
    }
    let summary = lines
        .iter()
        .find(|line| line.get("record").and_then(Value::as_str) == Some("summary"))
        .ok_or_else(|| crate::fail(format!("{name} summary")))?;
    expect_str(summary, "status", "succeeded")?;
    expect_bool(summary, "truncated", false)?;
    Ok(())
}

fn expect_cell(row: &Value, wanted: &[Cell]) -> Result<(), Fail> {
    let cells = row.get("cells").and_then(Value::as_array).ok_or_else(|| crate::fail("cells"))?;
    if cells.len() != wanted.len() {
        return Err(crate::fail("width"));
    }
    for (got, cell) in cells.iter().zip(wanted.iter()) {
        let ok = match cell {
            Cell::Null => got.get("kind").and_then(Value::as_str) == Some("null"),
            Cell::Bool(value) => {
                got.get("kind").and_then(Value::as_str) == Some("bool")
                    && got.get("value").and_then(|item| match item {
                        Value::Bool(bit) => Some(*bit),
                        _ => None,
                    }) == Some(*value)
            }
            Cell::Int(value) => {
                got.get("kind").and_then(Value::as_str) == Some("int")
                    && got.get("value").and_then(Value::as_str) == Some(*value)
            }
            Cell::Text(value) => {
                got.get("kind").and_then(Value::as_str) == Some("text")
                    && got.get("value").and_then(Value::as_str) == Some(*value)
            }
        };
        if !ok {
            return Err(crate::fail("cell"));
        }
    }
    Ok(())
}

fn check_audit(
    run: &Path,
    name: &str,
    role: &str,
    digest: &str,
    sql: &str,
    passwords: &[Vec<u8>],
) -> Result<(), Fail> {
    check_audit_file(&run.join("audit").join(format!("{name}.jsonl")), role, digest, sql, passwords)
}

fn check_audit_file(path: &Path, role: &str, digest: &str, sql: &str, passwords: &[Vec<u8>]) -> Result<(), Fail> {
    let raw = fs::read(path).map_err(|_| crate::fail(format!("audit {}", path.display())))?;
    if raw.windows(b"SELECT".len()).any(|window| window == b"SELECT") {
        return Err(crate::fail("audit stored SQL"));
    }
    for secret in passwords {
        if raw.windows(secret.len()).any(|window| window == secret.as_slice()) {
            return Err(crate::fail("audit stored a password"));
        }
    }
    let lines = json::lines(&raw)?;
    if lines.len() != 2 {
        return Err(crate::fail("audit records"));
    }
    expect_str(&lines[0], "record", "admit")?;
    expect_str(&lines[1], "record", "complete")?;
    expect_str(&lines[0], "protocol", "oo.gateway.query-pg-audit/v1")?;
    expect_str(&lines[0], "role", role)?;
    expect_str(&lines[0], "project", "pubfix")?;
    expect_str(&lines[0], "policy_id", digest)?;
    let hash = sha256_hex(sql.as_bytes());
    expect_str(&lines[0], "original_sha256", &hash)?;
    expect_str(&lines[1], "original_sha256", &hash)?;
    expect_str(&lines[1], "status", "succeeded")?;
    Ok(())
}

fn passwords(secrets: &Path) -> Result<Vec<Vec<u8>>, Fail> {
    ["admin.pass", "reader.pass", "runtime.pass", "login.pass", "writer.pass"]
        .into_iter()
        .map(|name| password(&secrets.join(name)))
        .collect()
}

fn password(path: &Path) -> Result<Vec<u8>, Fail> {
    let mut bytes = fs::read(path).map_err(|_| crate::fail("password file"))?;
    if bytes.ends_with(b"\r\n") {
        bytes.truncate(bytes.len() - 2);
    } else if bytes.ends_with(b"\n") || bytes.ends_with(b"\r") {
        bytes.pop();
    }
    if bytes.is_empty() {
        return Err(crate::fail("password empty"));
    }
    Ok(bytes)
}

fn absent_marker(path: &Path) -> Result<(), Fail> {
    let bytes = fs::read(path).map_err(|_| crate::fail(format!("read {}", path.display())))?;
    if bytes.windows(gold::MARKER.len()).any(|window| window == gold::MARKER.as_bytes()) {
        return Err(crate::fail(format!("{} contains the comment marker", path.display())));
    }
    Ok(())
}

fn expect_str(value: &Value, key: &str, wanted: &str) -> Result<(), Fail> {
    let got = value.get(key).and_then(Value::as_str).unwrap_or("");
    if got != wanted {
        return Err(crate::fail(format!("{key}={got}")));
    }
    Ok(())
}

fn expect_i64(value: &Value, key: &str, wanted: i64) -> Result<(), Fail> {
    let got = value.get(key).and_then(Value::as_i64);
    if got != Some(wanted) {
        return Err(crate::fail(format!("{key} mismatch")));
    }
    Ok(())
}

fn expect_bool(value: &Value, key: &str, wanted: bool) -> Result<(), Fail> {
    let got = value.get(key).and_then(|item| match item {
        Value::Bool(bit) => Some(*bit),
        _ => None,
    });
    if got != Some(wanted) {
        return Err(crate::fail(format!("{key} mismatch")));
    }
    Ok(())
}

fn pin(dir: &Path, name: &str) -> Result<String, Fail> {
    if !BINS.contains(&name) {
        return Err(crate::fail("bin"));
    }
    let path = dir.join(name);
    let digest = crate::sha::file_sha256_hex(&path)?;
    Ok(format!(
        "{{\"path\":{},\"sha256\":{}}}",
        json::escape(&path_text(&path)?),
        json::escape(&digest)
    ))
}

fn path_text(path: &Path) -> Result<String, Fail> {
    let text = path.to_str().ok_or_else(|| crate::fail("path"))?;
    if text.is_empty() || text.len() > 256 || !text.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':')) {
        return Err(crate::fail("path token"));
    }
    Ok(text.to_owned())
}
