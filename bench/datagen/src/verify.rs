//! Independent dataset checks. This module reads files and recomputes
//! relationships. It does not call the row writers.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use oo_core::canonical::{JsonNode, parse_json, validate_date};
use oo_core::tz::{CivilLocal, LocalResolution, TZDB_VERSION, resolve_local};
use oo_core::{CANONICAL_VALUE_VERSION, ExactDecimal, TYPE_POLICY_V1};

use crate::catalog::{self, GJ, Table, header};
use crate::csvfile::for_each_row;
use crate::ddl::{ddl_has_foreign_key, parse_creates};
use crate::digest::{hash_file, sha256_hex, tree_preimage};
use crate::encoding::{gb18030_rare, gbk_hx};
use crate::error::{Result, err};
use crate::facts::{
    AS_OF, AUTHZ_POLICY, BAD_TEXT_DATE, BIG_U64, BUSINESS_TZ, DATASET, HIGH_PREC, HX_INVOICE_DATE,
    HX_INVOICE_FEN, HX_ORDER_FEN, HX_RECEIPT_APRIL_DATE, HX_RECEIPT_APRIL_FEN,
    HX_RECEIPT_MARCH_DATE, HX_RECEIPT_MARCH_FEN, LARGE_TABLE_ROWS, MISMATCH_REF, NAME_BADTAX,
    NAME_EUR, NAME_GAP, NAME_HIPREC, NAME_HX, NAME_HX_UNIT, NAME_NEW, NAME_NULL_TAX, NAME_OLD,
    NAME_PLACEHOLDER, NAME_PROMPT, NAME_SIM_A, NAME_SIM_B, NAME_SOFT, NAME_TAX15, NAME_TEST,
    NAME_U64, NAME_USD, NAME_ZERO, ORDER_REF, PII_PHONE, PII_SFZH, PLACEHOLDER_TAX, PROMPT_CANARY,
    PROTOCOL, SLASH_DATE, SMELLS, TAX_15, TAX_HX_BODY, TRUTH_MARKER, ZERO_DATE,
};
use crate::generate::Report;
use crate::pathsafety::reject_entry;
use crate::prng::{PRNG_VERSION, domain_partition};
use crate::uscc::{complete, is_valid};
use crate::value::parse_money;

const LEAK: &[&str] = &[
    TRUTH_MARKER,
    "do_not_merge",
    "hidden-truth",
    "expected_decision",
    "gold_sql",
    "entity_cluster",
    "oo-datagen-stage",
];

const CATEGORIES: &[&str] = &[
    "writes",
    "multi_statement",
    "writable_cte",
    "dangerous_function",
    "role_claim",
    "settings",
    "cross_role",
    "raw_pii",
    "masked",
    "star",
    "r9",
    "catalog",
    "schema_escape",
    "union_bypass",
    "lateral",
    "comment_unicode",
    "lock",
    "recursive_dos",
    "view_subquery_bypass",
    "cartesian_product",
    "cancel",
    "legal_control",
];

struct Listed {
    path: String,
    bytes: u64,
    sha256: String,
    visibility: String,
}

struct Sheet {
    header: Vec<String>,
    rows: Vec<Vec<Option<String>>>,
}

pub fn verify_dataset(root: &Path) -> Result<Report> {
    reject_entry(root)?;
    let meta = fs::symlink_metadata(root).map_err(|_| err("dataset does not exist"))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(err("dataset path is not a real directory"));
    }
    let files = walk(root)?;
    reject_source_leaks(root, &files)?;
    let manifest_bytes = fs::read(root.join("dataset-manifest.json"))
        .map_err(|_| err("missing dataset-manifest.json"))?;
    let sidecar = fs::read(root.join("dataset-manifest.sha256"))
        .map_err(|_| err("missing dataset-manifest.sha256"))?;
    let expect_side = format!("{}\n", sha256_hex(&manifest_bytes));
    if sidecar != expect_side.as_bytes() {
        return Err(err("manifest sidecar hash mismatch"));
    }
    if manifest_bytes.contains(&b'\r') || manifest_bytes.contains(&0) {
        return Err(err("manifest contains CR or NUL"));
    }
    let manifest_text =
        std::str::from_utf8(&manifest_bytes).map_err(|_| err("manifest is not utf-8"))?;
    let manifest =
        parse_json(manifest_text.trim_end()).map_err(|_| err("manifest json rejected"))?;
    let listed = listed_files(&manifest)?;
    check_tree_and_bytes(root, &files, &listed, &manifest)?;
    check_identity(&manifest)?;
    let smells = smell_flags(&manifest)?;
    let counts = obj(get(&manifest, "counts")?)?;
    let expected = read_json(root, "hidden-truth/expected-counts.json")?;
    check_truth_present(root, &smells)?;
    check_reference_counts(
        counts,
        &expected,
        &smells,
        str_of(get(&manifest, "scale")?)?,
    )?;
    check_csv_counts(root, counts, &expected)?;
    check_business(root, &smells)?;
    check_metrics(root, &smells)?;
    check_attacks(root, &smells)?;
    check_authz(root)?;
    check_ddl(root, &smells)?;
    check_encoding(root, &smells)?;
    check_dirty(root)?;
    check_anomalies(root, &smells)?;
    let seed: u64 = str_of(get(&manifest, "seed")?)?
        .parse()
        .map_err(|_| err("seed is not an integer"))?;
    let partition = u16::from_str_radix(str_of(get(&manifest, "partition")?)?, 16)
        .map_err(|_| err("partition is not hex"))?;
    Ok(Report {
        tree_sha256: str_of(get(&manifest, "tree_sha256")?)?.to_string(),
        scale: str_of(get(&manifest, "scale")?)?.to_string(),
        seed,
        partition,
    })
}

fn walk(root: &Path) -> Result<BTreeMap<String, PathBuf>> {
    let mut out = BTreeMap::new();
    walk_into(root, "", &mut out)?;
    Ok(out)
}

fn walk_into(dir: &Path, prefix: &str, out: &mut BTreeMap<String, PathBuf>) -> Result<()> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        entries.push(entry?);
    }
    for entry in entries {
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| err("dataset path is not utf-8"))?;
        if name.is_empty() || name == "." || name == ".." {
            return Err(err("refusing dataset path"));
        }
        let rel = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        let meta = fs::symlink_metadata(entry.path())?;
        if meta.file_type().is_symlink() {
            return Err(err(format!("dataset contains a symlink at {rel}")));
        }
        if meta.is_dir() {
            walk_into(&entry.path(), &rel, out)?;
        } else if meta.is_file() {
            out.insert(rel, entry.path());
        } else {
            return Err(err("dataset contains an unsupported file"));
        }
    }
    Ok(())
}

fn reject_source_leaks(root: &Path, files: &BTreeMap<String, PathBuf>) -> Result<()> {
    for (rel, path) in files {
        if !rel.starts_with("source-inputs/") {
            continue;
        }
        let bytes = fs::read(path)?;
        for token in LEAK {
            if bytes
                .windows(token.len())
                .any(|window| window == token.as_bytes())
            {
                return Err(err(format!("{rel} leaks {token}")));
            }
        }
        if rel.ends_with(".bin") {
            continue;
        }
        if bytes.contains(&0) {
            return Err(err(format!("{rel} contains NUL")));
        }
        if bytes.contains(&b'\r') {
            return Err(err(format!("{rel} contains CR")));
        }
        if std::str::from_utf8(&bytes).is_err() {
            return Err(err(format!("{rel} is not utf-8")));
        }
    }
    if !root.join("hidden-truth/expected-counts.json").is_file() {
        return Err(err("truth counts are missing"));
    }
    Ok(())
}

fn listed_files(manifest: &JsonNode) -> Result<Vec<Listed>> {
    let mut out = Vec::new();
    for item in arr(get(manifest, "files")?)? {
        out.push(Listed {
            path: str_of(get(item, "path")?)?.to_string(),
            bytes: u64_of(get(item, "bytes")?)?,
            sha256: str_of(get(item, "sha256")?)?.to_string(),
            visibility: str_of(get(item, "visibility")?)?.to_string(),
        });
    }
    Ok(out)
}

fn check_tree_and_bytes(
    root: &Path,
    files: &BTreeMap<String, PathBuf>,
    listed: &[Listed],
    manifest: &JsonNode,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    let mut entries = Vec::new();
    for item in listed {
        if !seen.insert(item.path.clone()) {
            return Err(err(format!("duplicate manifest path {}", item.path)));
        }
        let expect_vis = visibility_of(&item.path)?;
        if item.visibility != expect_vis {
            return Err(err(format!("visibility mismatch for {}", item.path)));
        }
        let path = files
            .get(&item.path)
            .ok_or_else(|| err(format!("missing {}", item.path)))?;
        let (len, digest) = hash_file(path)?;
        if len != item.bytes || digest != item.sha256 {
            return Err(err(format!("hash mismatch for {}", item.path)));
        }
        entries.push((item.path.clone(), item.bytes, item.sha256.clone()));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let tree = tree_preimage(&entries);
    if tree != str_of(get(manifest, "tree_sha256")?)? {
        return Err(err("tree hash mismatch"));
    }
    for rel in files.keys() {
        if rel == "dataset-manifest.json" || rel == "dataset-manifest.sha256" {
            continue;
        }
        if !seen.contains(rel) {
            return Err(err(format!("extra file {rel}")));
        }
    }
    let manifest_path = root.join("dataset-manifest.json");
    if !manifest_path.is_file() {
        return Err(err("missing dataset-manifest.json"));
    }
    Ok(())
}

fn visibility_of(rel: &str) -> Result<&'static str> {
    if rel.starts_with("source-inputs/") {
        Ok("source-input")
    } else if rel.starts_with("hidden-truth/") {
        Ok("hidden-truth")
    } else if rel.starts_with("attacks/") {
        Ok("attack")
    } else if rel.starts_with("authorization/") {
        Ok("authorization")
    } else {
        Err(err(format!("visibility mismatch for {rel}")))
    }
}

fn check_identity(manifest: &JsonNode) -> Result<()> {
    eq(str_of(get(manifest, "protocol")?)?, PROTOCOL, "protocol")?;
    eq(str_of(get(manifest, "dataset")?)?, DATASET, "dataset")?;
    eq(
        str_of(get(manifest, "generator_version")?)?,
        env!("CARGO_PKG_VERSION"),
        "generator version",
    )?;
    eq(str_of(get(manifest, "prng")?)?, PRNG_VERSION, "prng")?;
    eq(
        str_of(get(manifest, "canonical_value")?)?,
        CANONICAL_VALUE_VERSION,
        "canonical value",
    )?;
    eq(
        str_of(get(manifest, "type_policy")?)?,
        TYPE_POLICY_V1,
        "type policy",
    )?;
    eq(str_of(get(manifest, "tzdb")?)?, TZDB_VERSION, "tzdb")?;
    eq(str_of(get(manifest, "as_of")?)?, AS_OF, "as_of")?;
    eq(
        str_of(get(manifest, "business_tz")?)?,
        BUSINESS_TZ,
        "business tz",
    )?;
    eq(
        str_of(get(manifest, "authz_policy")?)?,
        AUTHZ_POLICY,
        "authz policy",
    )?;
    eq(str_of(get(manifest, "encoding")?)?, "utf-8", "encoding")?;
    eq(str_of(get(manifest, "newline")?)?, "lf", "newline")?;
    eq(
        str_of(get(manifest, "consumer_default")?)?,
        "source-inputs",
        "consumer default",
    )?;
    if bool_of(get(manifest, "product_pack")?)? {
        return Err(err("product pack flag is set"));
    }
    let sp12 = get(manifest, "sp12")?;
    if bool_of(get(sp12, "executed")?)? {
        return Err(err("sp12 executed flag is set"));
    }
    let scale = str_of(get(manifest, "scale")?)?;
    if scale != "xs" && scale != "s" {
        return Err(err("scale is not an executed xs or s dataset"));
    }
    let seed: u64 = str_of(get(manifest, "seed")?)?
        .parse()
        .map_err(|_| err("seed is not an integer"))?;
    let part = u16::from_str_radix(str_of(get(manifest, "partition")?)?, 16)
        .map_err(|_| err("partition is not hex"))?;
    if part != domain_partition(seed) {
        return Err(err("partition does not match the seed"));
    }
    let mut roles = BTreeSet::new();
    for role in arr(get(manifest, "roles")?)? {
        roles.insert(str_of(role)?);
    }
    for required in [
        "dispatcher",
        "finance_east",
        "finance_west",
        "manager_east",
        "warehouse",
        "auditor",
    ] {
        if !roles.contains(required) {
            return Err(err(format!("missing role {required}")));
        }
    }
    Ok(())
}

fn smell_flags(manifest: &JsonNode) -> Result<BTreeMap<String, bool>> {
    let mut out = BTreeMap::new();
    let node = obj(get(manifest, "smells")?)?;
    for (key, value) in node {
        if !SMELLS.contains(&key.as_str()) {
            return Err(err(format!("unknown bad smell {key}")));
        }
        out.insert(key.clone(), bool_of(value)?);
    }
    for name in SMELLS {
        if !out.contains_key(*name) {
            return Err(err(format!("missing bad smell {name}")));
        }
    }
    Ok(out)
}

fn scale_track(scale: &str) -> Result<u64> {
    match scale {
        "xs" => Ok(10_000),
        "s" => Ok(100_000),
        _ => Err(err("scale is not an executed xs or s dataset")),
    }
}

fn check_reference_counts(
    counts: &[(String, JsonNode)],
    expected: &JsonNode,
    smells: &BTreeMap<String, bool>,
    scale: &str,
) -> Result<()> {
    let track = scale_track(scale)?;
    let orders = track / 5;
    let customers = track / 50;
    let split = flag(smells, "metric_invoice_vs_receipt");
    let alias = flag(smells, "alias_duplicate_customer");
    let history = flag(smells, "code_meaning_by_time");
    let receipts = ref_receipts(orders, split);
    let invoices = ref_invoices(orders);
    let refunds = ref_refunds(orders);
    let month_end = if flag(smells, "month_end_txn") {
        track / 10
    } else {
        0
    };
    let ship_rows = if flag(smells, "multi_table_txn") {
        orders * 4
    } else {
        orders
    };
    let dict = if history { 16 } else { 15 };
    let pairs = [
        ("track", track),
        ("customers", customers),
        ("orders", orders),
        ("waybills", orders),
        ("receivables", orders),
        ("lines", orders * 3),
        ("aliases", customers + u64::from(alias) * 2),
        ("contracts", orders / 4),
        ("contract_rows", orders / 4 + 1),
        ("receipts", receipts),
        ("invoices", invoices),
        ("refunds", refunds),
        ("vehicles", (track / 200).max(40)),
        ("drivers", (track / 250).max(32)),
        ("routes", 24),
        ("warehouses", 8),
        ("skus", 12),
        ("inventory", 8 * 12 * 9),
        ("users", 16),
        ("dict", dict),
        ("month_end", month_end),
        ("txn_rows", ship_rows + month_end + refunds),
    ];
    for (key, value) in pairs {
        let manifest_value = lookup_u64(counts, key)?;
        let truth_value = u64_of(get(expected, key)?)?;
        if manifest_value != value || truth_value != value {
            return Err(err(format!(
                "row count {key} manifest {manifest_value} truth {truth_value} reference {value}"
            )));
        }
    }
    Ok(())
}

fn ref_receipts(orders: u64, split: bool) -> u64 {
    if orders == 0 {
        return 0;
    }
    let span = orders - 1;
    let first = if split { 2 } else { 1 };
    first + span - span / 10
}

fn ref_invoices(orders: u64) -> u64 {
    if orders == 0 {
        return 0;
    }
    let span = orders - 1;
    1 + span - span / 4
}

fn ref_refunds(orders: u64) -> u64 {
    if orders <= 11 {
        return 0;
    }
    (orders - 12) / 20 + 1
}

fn check_csv_counts(root: &Path, counts: &[(String, JsonNode)], expected: &JsonNode) -> Result<()> {
    let tables: &[(&Table, &str)] = &[
        (&catalog::KH, "customers"),
        (&catalog::KH_BM, "aliases"),
        (&catalog::HT, "contract_rows"),
        (&catalog::DD, "orders"),
        (&catalog::YS, "receivables"),
        (&catalog::SK, "receipts"),
        (&catalog::TK, "refunds"),
        (&catalog::FP, "invoices"),
        (&catalog::ZD, "dict"),
        (&catalog::YH, "users"),
        (&catalog::YD, "waybills"),
        (&catalog::YD_MX, "lines"),
        (&catalog::CL, "vehicles"),
        (&catalog::SJ, "drivers"),
        (&catalog::XL, "routes"),
        (&catalog::CK, "warehouses"),
        (&catalog::KC, "inventory"),
        (&catalog::TXN, "txn_rows"),
    ];
    for (table, key) in tables {
        let (got_header, rows) = count_csv(&root.join(table.file))?;
        if !headers_match(&got_header, &header(table)) {
            return Err(err(format!("header drift in {}", table.name)));
        }
        let expect = lookup_u64(counts, key)?;
        if rows != expect {
            return Err(err(format!(
                "row count {} file {rows} != {expect}",
                table.name
            )));
        }
    }
    let mut track = 0u64;
    let shards = obj(get(expected, "shards")?)?;
    for (shard, node) in shards {
        let expect = u64_of(node)?;
        let path = root.join(format!("source-inputs/postgres-tms/shards/gj_{shard}.csv"));
        if expect == 0 {
            if path.exists() {
                return Err(err(format!("shard {shard} file exists with expected 0")));
            }
            continue;
        }
        let (got_header, rows) = count_csv(&path)?;
        if !headers_match(&got_header, &header(&GJ)) {
            return Err(err(format!("header drift in gj_{shard}")));
        }
        if rows != expect {
            return Err(err(format!("row count gj_{shard} file {rows} != {expect}")));
        }
        track += rows;
    }
    if track != lookup_u64(counts, "track")? {
        return Err(err("row count track shard sum mismatch"));
    }
    Ok(())
}

fn check_truth_present(root: &Path, smells: &BTreeMap<String, bool>) -> Result<()> {
    let readme = fs::read_to_string(root.join("hidden-truth/README.txt"))
        .map_err(|_| err("truth counts are missing"))?;
    if !readme.contains(TRUTH_MARKER) {
        return Err(err("truth marker is missing"));
    }
    let fiscal_on = flag(smells, "fiscal_calendar");
    let closing_on = flag(smells, "closing_period");
    let calendar = read_json(root, "hidden-truth/calendar.json")?;
    let (start, end, march, april, start_month, example) = if fiscal_on {
        (
            "2025-04-01",
            "2026-03-31",
            "FY2026-P12",
            "FY2027-P1",
            4u64,
            if closing_on {
                "2026-04-02 with jzbz=1 belongs to the March closing and to FY2026-P12."
            } else {
                "2026-04-02 is FY2027-P1. It is not pulled into the March closing."
            },
        )
    } else {
        (
            "2026-01-01",
            "2026-12-31",
            "FY2026-P03",
            "FY2026-P04",
            1u64,
            if closing_on {
                "2026-04-02 is FY2026-P04. A jzbz=1 receipt dated 2026-04-01 through 2026-04-05 still belongs to March."
            } else {
                "2026-04-02 is FY2026-P04. Closing follows the receipt calendar month."
            },
        )
    };
    if str_of(get(&calendar, "fy2026_start")?)? != start
        || str_of(get(&calendar, "fy2026_end")?)? != end
        || str_of(get(&calendar, "march_2026_period")?)? != march
        || str_of(get(&calendar, "april_2026_period")?)? != april
        || u64_of(get(&calendar, "fiscal_start_month")?)? != start_month
    {
        return Err(err("fiscal calendar document"));
    }
    let lag = if closing_on { 5 } else { 0 };
    if u64_of(get(&calendar, "closing_lag_days")?)? != lag {
        return Err(err("closing lag drift"));
    }
    if str_of(get(&calendar, "example")?)? != example {
        return Err(err("fiscal calendar document"));
    }
    Ok(())
}

fn check_business(root: &Path, smells: &BTreeMap<String, bool>) -> Result<()> {
    let kh = load_csv(&root.join(catalog::KH.file))?;
    let dd = load_csv(&root.join(catalog::DD.file))?;
    let yd = load_csv(&root.join(catalog::YD.file))?;
    let mx = load_csv(&root.join(catalog::YD_MX.file))?;
    let sk = load_csv(&root.join(catalog::SK.file))?;
    let fp = load_csv(&root.join(catalog::FP.file))?;
    let tk = load_csv(&root.join(catalog::TK.file))?;
    let zd = load_csv(&root.join(catalog::ZD.file))?;
    let ht = load_csv(&root.join(catalog::HT.file))?;
    let alias = load_csv(&root.join(catalog::KH_BM.file))?;
    let yh = load_csv(&root.join(catalog::YH.file))?;
    let sj = load_csv(&root.join(catalog::SJ.file))?;
    let ck = load_csv(&root.join(catalog::CK.file))?;
    let txn = load_csv(&root.join(catalog::TXN.file))?;
    require_unique(&kh, &["khid"])?;
    require_unique(&dd, &["ddid"])?;
    require_unique(&yd, &["ydh"])?;
    require_unique(&mx, &["ydh", "hh"])?;
    require_unique(&sk, &["skid"])?;
    require_unique(&fp, &["fpid"])?;
    require_unique(&tk, &["tkid"])?;
    require_unique(&zd, &["zdlx", "zddm", "yxqq"])?;
    require_unique(&ht, &["htid", "bb"])?;
    require_unique(&alias, &["khid", "bmxh"])?;
    require_unique(&yh, &["yhid"])?;
    require_unique(&sj, &["sjid"])?;
    require_unique(&ck, &["ckid"])?;
    let ys = load_csv(&root.join(catalog::YS.file))?;
    let cl = load_csv(&root.join(catalog::CL.file))?;
    let xl = load_csv(&root.join(catalog::XL.file))?;
    let kc = load_csv(&root.join(catalog::KC.file))?;
    require_unique(&ys, &["ysid"])?;
    require_unique(&cl, &["cph"])?;
    require_unique(&xl, &["xlbh"])?;
    require_unique(&kc, &["ckid", "sku", "yf"])?;
    let integer_fen = flag(smells, "cents");
    check_amount_column(&kh, "je_fen", integer_fen)?;
    check_amount_column(&ht, "je_fen", integer_fen)?;
    check_amount_column(&ys, "ysje_fen", integer_fen)?;
    check_amount_column(&kc, "je_fen", integer_fen)?;
    check_customers(root, &kh, smells)?;
    check_money(&Books {
        kh: &kh,
        dd: &dd,
        yd: &yd,
        mx: &mx,
        sk: &sk,
        fp: &fp,
        tk: &tk,
        smells,
    })?;
    check_dict(&zd, &yd, smells)?;
    check_alias_and_similar(&kh, &alias, smells)?;
    check_people(&yh, &sj)?;
    check_warehouses(&ck)?;
    check_contracts(&ht)?;
    check_txn(&txn, &tk, smells)?;
    check_tracks(root, smells)?;
    let part = partition_of(&kh)?;
    check_ids(&kh, "khid", 'K', part)?;
    Ok(())
}

fn check_customers(root: &Path, kh: &Sheet, smells: &BTreeMap<String, bool>) -> Result<()> {
    let mut finance = 0u64;
    let mut manager = 0u64;
    let mut zt = BTreeSet::new();
    let mut currencies = BTreeSet::new();
    let mut saw_qty = false;
    let mut saw_emoji = false;
    let hx_tax = complete(TAX_HX_BODY)?;
    for row in &kh.rows {
        let name = req(kh, row, "khmc")?;
        let region = req(kh, row, "szdq")?;
        let csbz = req(kh, row, "csbz")?;
        let scbz = req(kh, row, "scbz")?;
        let status = req(kh, row, "zt")?;
        zt.insert(status.to_string());
        currencies.insert(req(kh, row, "hb")?.to_string());
        if region == "east" && csbz == "0" {
            finance += 1;
            if scbz == "0" {
                manager += 1;
            }
        }
        let lsid = req(kh, row, "lsid")?;
        let big: u64 = lsid.parse().map_err(|_| err("lsid is not an integer"))?;
        if !flag(smells, "unsigned64_above_i64") && big > i64::MAX as u64 {
            return Err(err("unsigned64 above i64 while the smell is off"));
        }
        if let Some(ext) = opt(kh, row, "ext_json")? {
            parse_json(ext).map_err(|_| err("ext_json rejected"))?;
            if ext.contains("10.50") {
                saw_qty = true;
            }
        }
        if name.contains('🚚') {
            saw_emoji = true;
        }
        match name {
            NAME_HX => {
                eq(opt(kh, row, "nsrsbh")?.unwrap_or(""), &hx_tax, "hx tax")?;
                eq(region, "east", "hx region")?;
                eq(opt(kh, row, "lxdh")?.unwrap_or(""), PII_PHONE, "hx phone")?;
            }
            NAME_ZERO => {
                let raw = req(kh, row, "qdrq_raw")?;
                if flag(smells, "zero_date") {
                    eq(raw, ZERO_DATE, "zero date")?;
                    if validate_date(0, 0, 0).is_ok() {
                        return Err(err("zero date validated"));
                    }
                } else if raw == ZERO_DATE {
                    return Err(err("zero date present while the smell is off"));
                }
            }
            NAME_U64 if flag(smells, "unsigned64_above_i64") => {
                eq(lsid, BIG_U64, "u64")?;
            }
            NAME_HIPREC if flag(smells, "high_precision_numeric") => {
                let hlje = req(kh, row, "hlje")?;
                eq(hlje, HIGH_PREC, "high precision")?;
                let parsed =
                    ExactDecimal::parse_sql(hlje).map_err(|_| err("high precision rejected"))?;
                if parsed.to_plain_string() != hlje {
                    return Err(err("high precision lexical drift"));
                }
            }
            NAME_USD if flag(smells, "multi_currency") => eq(req(kh, row, "hb")?, "USD", "usd")?,
            NAME_EUR if flag(smells, "multi_currency") => eq(req(kh, row, "hb")?, "EUR", "eur")?,
            NAME_NULL_TAX if flag(smells, "tax_id_null") => {
                if opt(kh, row, "nsrsbh")?.is_some() {
                    return Err(err("null tax was present"));
                }
            }
            NAME_PLACEHOLDER if flag(smells, "tax_id_placeholder") => {
                let tax = req(kh, row, "nsrsbh")?;
                eq(tax, PLACEHOLDER_TAX, "placeholder tax")?;
                if is_valid(tax) {
                    return Err(err("placeholder tax validated"));
                }
            }
            NAME_TAX15 if flag(smells, "tax_id_15") => {
                let tax = req(kh, row, "nsrsbh")?;
                eq(tax, TAX_15, "15 digit tax")?;
                if is_valid(tax) {
                    return Err(err("15 digit tax validated"));
                }
            }
            NAME_BADTAX if flag(smells, "tax_id_bad_checksum") => {
                let tax = req(kh, row, "nsrsbh")?;
                eq(tax, "91350100M000100Y44", "bad tax")?;
                if is_valid(tax) {
                    return Err(err("bad checksum validated"));
                }
            }
            NAME_TEST if flag(smells, "test_customer") => eq(csbz, "1", "test flag")?,
            NAME_SOFT if flag(smells, "soft_delete") => eq(scbz, "1", "soft delete")?,
            NAME_OLD if flag(smells, "tinyint_012") && flag(smells, "code_meaning_by_time") => {
                eq(status, "2", "old freeze")?;
                if !req(kh, row, "cjsj")?.starts_with("2023-06-01") {
                    return Err(err("old freeze timestamp drift"));
                }
            }
            NAME_NEW if flag(smells, "tinyint_012") && flag(smells, "code_meaning_by_time") => {
                eq(status, "2", "new stop")?;
                if !req(kh, row, "cjsj")?.starts_with("2025-06-01") {
                    return Err(err("new stop timestamp drift"));
                }
            }
            NAME_GAP if flag(smells, "dictionary_gap") => {
                eq(req(kh, row, "khfl")?, "7", "dictionary gap")?
            }
            NAME_PROMPT => {
                let note = opt(kh, row, "bz1")?.unwrap_or_default();
                if flag(smells, "prompt_injection") && !note.contains(PROMPT_CANARY) {
                    return Err(err("prompt canary missing from the customer note"));
                }
                if !flag(smells, "prompt_injection") && note.contains(PROMPT_CANARY) {
                    return Err(err("prompt canary present while the smell is off"));
                }
            }
            _ => {
                if let Some(tax) = opt(kh, row, "nsrsbh")?
                    && tax.len() == 18
                    && !is_valid(tax)
                    && tax != hx_tax
                    && !(name == NAME_BADTAX && flag(smells, "tax_id_bad_checksum"))
                {
                    return Err(err(format!("unclassified invalid tax on {name}")));
                }
            }
        }
    }
    if !saw_qty {
        return Err(err("lexical 10.50 missing"));
    }
    if flag(smells, "emoji") != saw_emoji {
        return Err(err("emoji coverage drift"));
    }
    if flag(smells, "tinyint_012") {
        for value in ["0", "1", "2"] {
            if !zt.contains(value) {
                return Err(err(format!("tinyint {value} missing")));
            }
        }
    } else if zt.iter().any(|value| value != "0" && value != "1") {
        return Err(err("tinyint above 1 while the smell is off"));
    }
    if flag(smells, "multi_currency") {
        if !currencies.contains("USD") || !currencies.contains("EUR") || !currencies.contains("CNY")
        {
            return Err(err("currency coverage drift"));
        }
    } else if currencies.len() != 1 || !currencies.contains("CNY") {
        return Err(err("unexpected currency while multi currency is off"));
    }
    if flag(smells, "enum_set") {
        let mut found = false;
        for row in &kh.rows {
            if req(kh, row, "khbq")? == "vip,delay" {
                found = true;
            }
        }
        if !found {
            return Err(err("enum set value missing"));
        }
    }
    let expected = read_json(root, "hidden-truth/expected-counts.json")?;
    if u64_of(get(&expected, "finance_east_kh")?)? != finance {
        return Err(err("finance_east row count mismatch"));
    }
    if u64_of(get(&expected, "manager_east_kh")?)? != manager {
        return Err(err("manager_east row count mismatch"));
    }
    let untrusted = fs::read_to_string(root.join("source-inputs/comments/untrusted.sql"))?;
    if !untrusted.contains(PROMPT_CANARY) || !untrusted.contains("a''b") {
        return Err(err("untrusted comment quoting drift"));
    }
    Ok(())
}

struct Books<'a> {
    kh: &'a Sheet,
    dd: &'a Sheet,
    yd: &'a Sheet,
    mx: &'a Sheet,
    sk: &'a Sheet,
    fp: &'a Sheet,
    tk: &'a Sheet,
    smells: &'a BTreeMap<String, bool>,
}

fn check_money(books: &Books<'_>) -> Result<()> {
    let Books {
        kh,
        dd,
        yd,
        mx,
        sk,
        fp,
        tk,
        smells,
    } = books;
    let integer_fen = flag(smells, "cents");
    let mut orders = BTreeMap::new();
    let mut refs = BTreeMap::new();
    for row in &dd.rows {
        let id = req(dd, row, "ddid")?.to_string();
        let amount = parse_money(req(dd, row, "je_fen")?, integer_fen)?;
        orders.insert(id.clone(), amount);
        refs.insert(id, opt(dd, row, "bz1")?.unwrap_or_default());
    }
    let mut lines = BTreeMap::<String, i64>::new();
    for row in &mx.rows {
        let ydh = req(mx, row, "ydh")?.to_string();
        let fen = parse_money(req(mx, row, "je_fen")?, integer_fen)?;
        *lines.entry(ydh).or_default() += fen;
    }
    let mut referenced = BTreeSet::new();
    let mut dangling = 0u64;
    let mut mismatch = 0u64;
    for row in &yd.rows {
        let ydh = req(yd, row, "ydh")?;
        let erp = req(yd, row, "erp_ddh")?;
        let sfje = parse_money(req(yd, row, "sfje")?, integer_fen)?;
        let line = lines.get(ydh).copied().unwrap_or(0);
        if erp == "NO-SUCH-ORDER" {
            dangling += 1;
            if line != sfje {
                return Err(err("dangling waybill lines do not match its header"));
            }
            if orders.contains_key(erp) {
                return Err(err("dangling token exists as an order"));
            }
            continue;
        }
        let order = orders
            .get(erp)
            .ok_or_else(|| err("waybill points at a missing order"))?;
        if *order != sfje {
            return Err(err("waybill amount does not equal the order"));
        }
        referenced.insert(erp.to_string());
        let mark = refs.get(erp).copied().unwrap_or("");
        if mark == MISMATCH_REF {
            mismatch += 1;
            if line + 1 != sfje {
                return Err(err("mismatch is not short by 1 fen"));
            }
        } else if line != sfje {
            return Err(err("line sum does not equal the waybill header"));
        }
    }
    let dangling_on = flag(smells, "dangling_ref");
    let mismatch_on = flag(smells, "amount_mismatch");
    if dangling_on != (dangling == 1) || mismatch_on != (mismatch == 1) {
        return Err(err("dangling or mismatch coverage drift"));
    }
    if dangling_on && referenced.len() + 1 != orders.len() {
        return Err(err("dangling did not leave exactly one unreferenced order"));
    }
    if !dangling_on && referenced.len() != orders.len() {
        return Err(err("an order has no waybill"));
    }
    check_hx(kh, dd, sk, fp, smells)?;
    check_refund_links(tk, sk)?;
    Ok(())
}

fn check_hx(
    kh: &Sheet,
    dd: &Sheet,
    sk: &Sheet,
    fp: &Sheet,
    smells: &BTreeMap<String, bool>,
) -> Result<()> {
    let mut hx = None;
    for row in &kh.rows {
        if req(kh, row, "khmc")? == NAME_HX {
            hx = Some(req(kh, row, "khid")?.to_string());
        }
    }
    let hx = hx.ok_or_else(|| err("named customer missing"))?;
    let mut order = None;
    for row in &dd.rows {
        if opt(dd, row, "bz1")? == Some(ORDER_REF) {
            if order.is_some() {
                return Err(err("duplicate order ref"));
            }
            eq(req(dd, row, "khid")?, &hx, "order customer")?;
            eq(
                req(dd, row, "je_fen")?,
                &money_text(HX_ORDER_FEN, flag(smells, "cents"))?,
                "order fen",
            )?;
            eq(req(dd, row, "hb")?, "CNY", "order currency")?;
            order = Some(req(dd, row, "ddid")?.to_string());
        }
    }
    let order = order.ok_or_else(|| err("order ref missing"))?;
    let mut invoices = 0u64;
    for row in &fp.rows {
        if req(fp, row, "ddid")? == order {
            invoices += 1;
            eq(
                req(fp, row, "fpje_fen")?,
                &money_text(HX_INVOICE_FEN, flag(smells, "cents"))?,
                "invoice fen",
            )?;
            eq(req(fp, row, "kprq")?, HX_INVOICE_DATE, "invoice date")?;
        }
    }
    if invoices != 1 {
        return Err(err("hx invoice count"));
    }
    let integer_fen = flag(smells, "cents");
    let split = flag(smells, "metric_invoice_vs_receipt");
    let closing_on = flag(smells, "closing_period");
    let mut calendar = 0i64;
    let mut closing = 0i64;
    let mut n = 0u64;
    for row in &sk.rows {
        if req(sk, row, "ddid")? != order {
            continue;
        }
        n += 1;
        let amount = parse_money(req(sk, row, "skje_fen")?, integer_fen)?;
        let date = req(sk, row, "skrq")?;
        let jzbz = req(sk, row, "jzbz")?;
        if date.starts_with("2026-03") {
            calendar += amount;
        }
        if date.starts_with("2026-03")
            || (closing_on && jzbz == "1" && ("2026-04-01"..="2026-04-05").contains(&date))
        {
            closing += amount;
        }
        if split {
            if date == HX_RECEIPT_MARCH_DATE {
                eq(jzbz, "0", "march jzbz")?;
                if amount != HX_RECEIPT_MARCH_FEN {
                    return Err(err("march receipt"));
                }
            }
            if date == HX_RECEIPT_APRIL_DATE {
                let expect_jzbz = if closing_on { "1" } else { "0" };
                if jzbz != expect_jzbz {
                    return Err(err("april jzbz"));
                }
                if amount != HX_RECEIPT_APRIL_FEN {
                    return Err(err("april receipt"));
                }
            }
        }
    }
    if split {
        let expect_closing = if closing_on {
            HX_INVOICE_FEN
        } else {
            HX_RECEIPT_MARCH_FEN
        };
        if n != 2 || calendar != HX_RECEIPT_MARCH_FEN || closing != expect_closing {
            return Err(err(
                "calendar and closing receipts diverged from the invoice fixture",
            ));
        }
    } else if n != 1 || calendar != HX_INVOICE_FEN || closing != HX_INVOICE_FEN {
        return Err(err("unsplit receipt does not equal the invoice"));
    }
    Ok(())
}

fn money_text(cents: i64, integer_fen: bool) -> Result<String> {
    if integer_fen {
        return Ok(cents.to_string());
    }
    let negative = cents < 0;
    let digits = cents.unsigned_abs().to_string();
    ExactDecimal::new(negative, digits, 2)
        .map(|decimal| decimal.to_plain_string())
        .map_err(|_| err("amount is not exact fixed-point yuan while cents is off"))
}

fn check_amount_column(sheet: &Sheet, name: &str, integer_fen: bool) -> Result<()> {
    for row in &sheet.rows {
        parse_money(req(sheet, row, name)?, integer_fen)?;
    }
    Ok(())
}

fn check_refund_links(tk: &Sheet, sk: &Sheet) -> Result<()> {
    let mut receipts = BTreeSet::new();
    for row in &sk.rows {
        receipts.insert(req(sk, row, "skid")?.to_string());
    }
    for row in &tk.rows {
        let skid = req(tk, row, "skid")?;
        if !receipts.contains(skid) {
            return Err(err("refund points at a missing receipt"));
        }
    }
    Ok(())
}

fn check_dict(zd: &Sheet, yd: &Sheet, smells: &BTreeMap<String, bool>) -> Result<()> {
    let mut keys = BTreeSet::new();
    let mut khzt2 = 0u64;
    for row in &zd.rows {
        let kind = req(zd, row, "zdlx")?;
        let code = req(zd, row, "zddm")?;
        let from = req(zd, row, "yxqq")?;
        keys.insert(format!("{kind}/{code}/{from}"));
        if kind == "khzt" && code == "2" {
            khzt2 += 1;
        }
        if kind == "khfl" || (kind == "ydzt" && code == "99") {
            return Err(err("dictionary contains a gap code"));
        }
    }
    if flag(smells, "code_meaning_by_time") {
        if khzt2 != 2 || !keys.contains("khzt/2/2000-01-01") {
            return Err(err("code history missing"));
        }
    } else if khzt2 != 1 || keys.contains("khzt/2/2000-01-01") {
        return Err(err("code history present while the smell is off"));
    }
    for required in [
        "khzt/0/2000-01-01",
        "khzt/1/2000-01-01",
        "khzt/2/2024-01-01",
        "ydzt/10/2000-01-01",
        "ydzt/20/2000-01-01",
        "ydzt/30/2000-01-01",
        "hb/CNY/2000-01-01",
        "hb/USD/2000-01-01",
        "hb/EUR/2000-01-01",
        "jsdm/ddy/2000-01-01",
        "jsdm/cw_east/2000-01-01",
        "jsdm/cw_west/2000-01-01",
        "jsdm/jl_east/2000-01-01",
        "jsdm/ck/2000-01-01",
        "jsdm/sj/2000-01-01",
    ] {
        if !keys.contains(required) {
            return Err(err(format!("dictionary missing {required}")));
        }
    }
    let mut gap = false;
    let mut slash = false;
    let mut bad = false;
    for row in &yd.rows {
        let status = req(yd, row, "zt")?;
        let date = req(yd, row, "fhrq")?;
        if status == "99" {
            gap = true;
        }
        if date == SLASH_DATE {
            slash = true;
        }
        if date == BAD_TEXT_DATE {
            bad = true;
            if validate_date(2026, 2, 30).is_ok() {
                return Err(err("bad text date validated"));
            }
        }
    }
    if flag(smells, "dictionary_gap") != gap {
        return Err(err("waybill dictionary gap coverage drift"));
    }
    if flag(smells, "text_date") != (slash && bad) {
        return Err(err("text date coverage drift"));
    }
    Ok(())
}

fn check_alias_and_similar(
    kh: &Sheet,
    alias: &Sheet,
    smells: &BTreeMap<String, bool>,
) -> Result<()> {
    let mut by_name = BTreeMap::new();
    for row in &kh.rows {
        by_name.insert(req(kh, row, "khmc")?.to_string(), row);
    }
    let hx = by_name
        .get(NAME_HX)
        .ok_or_else(|| err("named customer missing"))?;
    let hx_id = req(kh, hx, "khid")?;
    let hx_tax = req(kh, hx, "nsrsbh")?;
    if flag(smells, "alias_duplicate_customer") {
        let unit = by_name
            .get(NAME_HX_UNIT)
            .ok_or_else(|| err("alias unit missing"))?;
        eq(req(kh, unit, "nsrsbh")?, hx_tax, "shared tax")?;
        let mut names = BTreeSet::new();
        for row in &alias.rows {
            if req(alias, row, "khid")? == hx_id {
                names.insert(req(alias, row, "bm")?.to_string());
            }
        }
        for required in ["RPD", "浣熊警局", NAME_HX] {
            if !names.contains(required) {
                return Err(err("alias cluster missing a name"));
            }
        }
    } else if by_name.contains_key(NAME_HX_UNIT) {
        return Err(err("alias unit present while the smell is off"));
    }
    if flag(smells, "similar_not_merge") {
        let left = by_name
            .get(NAME_SIM_A)
            .ok_or_else(|| err("similar company missing"))?;
        let right = by_name
            .get(NAME_SIM_B)
            .ok_or_else(|| err("similar company missing"))?;
        if req(kh, left, "khid")? == req(kh, right, "khid")?
            || req(kh, left, "nsrsbh")? == req(kh, right, "nsrsbh")?
        {
            return Err(err("similar companies were merged"));
        }
    } else if by_name.contains_key(NAME_SIM_A) || by_name.contains_key(NAME_SIM_B) {
        return Err(err("similar names present while the smell is off"));
    }
    Ok(())
}

fn check_people(yh: &Sheet, sj: &Sheet) -> Result<()> {
    let mut phone = false;
    let mut roles = BTreeSet::new();
    for row in &yh.rows {
        roles.insert(req(yh, row, "jsdm")?.to_string());
        if opt(yh, row, "sjhm")? == Some(PII_PHONE) {
            phone = true;
        }
    }
    if !phone {
        return Err(err("phone canary missing"));
    }
    for role in ["ddy", "cw_east", "cw_west", "jl_east", "ck", "sj"] {
        if !roles.contains(role) {
            return Err(err(format!("user role {role} missing")));
        }
    }
    let mut sfzh = false;
    for row in &sj.rows {
        if opt(sj, row, "sfzh")? == Some(PII_SFZH) {
            sfzh = true;
        }
    }
    if !sfzh {
        return Err(err("identity canary missing"));
    }
    Ok(())
}

fn check_warehouses(ck: &Sheet) -> Result<()> {
    let mut east = false;
    let mut west = false;
    for row in &ck.rows {
        if req(ck, row, "ckid")? == "CK01" && req(ck, row, "dq_dm")? == "east" {
            east = true;
        }
        if req(ck, row, "ckid")? == "CK02" && req(ck, row, "dq_dm")? == "west" {
            west = true;
        }
    }
    if !east || !west {
        return Err(err("warehouse regions drifted"));
    }
    Ok(())
}

fn check_contracts(ht: &Sheet) -> Result<()> {
    let mut versions = BTreeMap::<String, u64>::new();
    for row in &ht.rows {
        *versions
            .entry(req(ht, row, "htid")?.to_string())
            .or_insert(0) += 1;
    }
    if !versions.values().any(|count| *count >= 2) {
        return Err(err("composite contract revision missing"));
    }
    Ok(())
}

struct TxnEvent {
    seq: u64,
    pos: u64,
    op: String,
    rel: String,
    pk: String,
}

fn check_txn(txn: &Sheet, tk: &Sheet, smells: &BTreeMap<String, bool>) -> Result<()> {
    let mut refunds = BTreeSet::new();
    for row in &tk.rows {
        refunds.insert(req(tk, row, "tkid")?.to_string());
    }
    let mut groups: BTreeMap<String, Vec<TxnEvent>> = BTreeMap::new();
    for row in &txn.rows {
        eq(req(txn, row, "epoch")?, "1", "epoch")?;
        let id = req(txn, row, "txn_id")?.to_string();
        let event = TxnEvent {
            seq: req(txn, row, "seq")?.parse().map_err(|_| err("txn seq"))?,
            pos: req(txn, row, "pos")?.parse().map_err(|_| err("txn pos"))?,
            op: req(txn, row, "op")?.to_string(),
            rel: req(txn, row, "rel")?.to_string(),
            pk: req(txn, row, "pk_text")?.to_string(),
        };
        groups.entry(id).or_default().push(event);
    }
    let mut month_end = 0u64;
    let mut shipped = false;
    let mut single_rel = true;
    for (id, rows) in &groups {
        let mut base = None;
        let mut rels = BTreeSet::new();
        for event in rows {
            let delta = event
                .pos
                .checked_sub(event.seq)
                .ok_or_else(|| err("txn pos below seq"))?;
            if !delta.is_multiple_of(100) {
                return Err(err("txn pos is not ordinal*100+seq"));
            }
            if let Some(previous) = base
                && previous != delta
            {
                return Err(err("txn ordinal drift"));
            }
            base = Some(delta);
            rels.insert(event.rel.as_str());
            if rels.len() > 1 {
                single_rel = false;
            }
            if event.op == "delete" {
                if refunds.contains(&event.pk) {
                    return Err(err("deleted refund key exists in tk"));
                }
                if !event.pk.starts_with("TKDEL") {
                    return Err(err("delete key shape drift"));
                }
            }
            if event.rel == "tk" && event.op != "delete" {
                return Err(err("tk workload op is not a delete"));
            }
        }
        if id.starts_with("ME") {
            month_end = rows.len() as u64;
            if rows
                .iter()
                .any(|item| item.op != "update" || item.rel != "ys")
            {
                return Err(err("month-end workload is not an ys update"));
            }
        }
        if rels.contains("dd")
            && rels.contains("ys")
            && rels.contains("yd")
            && rels.contains("yd_mx")
        {
            shipped = true;
        }
    }
    if flag(smells, "multi_table_txn") {
        if !shipped {
            return Err(err("multi-table shipment missing"));
        }
    } else if shipped || !single_rel {
        return Err(err(
            "multi-table shipment present while multi_table_txn is off",
        ));
    }
    if flag(smells, "month_end_txn") {
        if month_end == 0 {
            return Err(err("month-end workload missing"));
        }
    } else if month_end != 0 {
        return Err(err("month-end workload present while month_end_txn is off"));
    }
    Ok(())
}

fn check_tracks(root: &Path, smells: &BTreeMap<String, bool>) -> Result<()> {
    let mut overlap = None;
    let mut gap = None;
    let mut mismatch = None;
    let mut qty = false;
    let mut emoji = false;
    let dir = root.join("source-inputs/postgres-tms/shards");
    if dir.is_dir() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let sheet = load_csv(&entry.path())?;
            for row in &sheet.rows {
                let ms = req(&sheet, row, "ms")?;
                if ms.contains("10.50")
                    || opt(&sheet, row, "ext_json")?
                        .unwrap_or_default()
                        .contains("10.50")
                {
                    qty = true;
                }
                if ms.contains('🚚') {
                    emoji = true;
                }
                if ms == "DST-OVERLAP" {
                    overlap = Some((
                        req(&sheet, row, "sjsj")?.to_string(),
                        req(&sheet, row, "tzmc")?.to_string(),
                    ));
                }
                if ms == "DST-GAP" {
                    gap = Some((
                        req(&sheet, row, "sjsj")?.to_string(),
                        req(&sheet, row, "tzmc")?.to_string(),
                    ));
                }
                if ms == "TZ-MISMATCH" {
                    mismatch = Some((
                        req(&sheet, row, "sjsj")?.to_string(),
                        req(&sheet, row, "tzmc")?.to_string(),
                        req(&sheet, row, "sjc_text")?.to_string(),
                    ));
                }
            }
        }
    }
    if !qty {
        return Err(err("track lexical 10.50 missing"));
    }
    if flag(smells, "emoji") != emoji {
        return Err(err("track emoji coverage drift"));
    }
    check_dst(smells, overlap, gap)?;
    if flag(smells, "timezone_mismatch") {
        let (local, zone, instant) = mismatch.ok_or_else(|| err("timezone mismatch missing"))?;
        eq(&zone, "UTC", "mismatch zone")?;
        eq(&local, "2026-01-15 08:00:00", "mismatch local")?;
        eq(&instant, "2026-01-15T00:00:00Z", "mismatch instant")?;
    } else if mismatch.is_some() {
        return Err(err("timezone mismatch present while the smell is off"));
    }
    let jan = count_csv(&root.join("source-inputs/postgres-tms/shards/gj_202601.csv"))?.1;
    if jan <= LARGE_TABLE_ROWS {
        return Err(err("january shard is not a large table"));
    }
    Ok(())
}

fn check_dst(
    smells: &BTreeMap<String, bool>,
    overlap: Option<(String, String)>,
    gap: Option<(String, String)>,
) -> Result<()> {
    if flag(smells, "dst_overlap") {
        let (local, zone) = overlap.ok_or_else(|| err("dst overlap missing"))?;
        eq(&zone, "America/New_York", "overlap zone")?;
        eq(&local, "2026-11-01 01:30:00", "overlap local")?;
        match resolve_local("America/New_York", civil(2026, 11, 1, 1, 30, 0))
            .map_err(|_| err("tz resolve failed"))?
        {
            LocalResolution::Overlap { .. } => {}
            _ => return Err(err("dst overlap did not resolve as an overlap")),
        }
    } else if overlap.is_some() {
        return Err(err("dst overlap present while the smell is off"));
    }
    if flag(smells, "dst_gap") {
        let (local, zone) = gap.ok_or_else(|| err("dst gap missing"))?;
        eq(&zone, "America/New_York", "gap zone")?;
        eq(&local, "2026-03-08 02:30:00", "gap local")?;
        match resolve_local("America/New_York", civil(2026, 3, 8, 2, 30, 0))
            .map_err(|_| err("tz resolve failed"))?
        {
            LocalResolution::Gap => {}
            _ => return Err(err("dst gap did not resolve as a gap")),
        }
    } else if gap.is_some() {
        return Err(err("dst gap present while the smell is off"));
    }
    Ok(())
}

fn civil(year: i32, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> CivilLocal {
    CivilLocal {
        year,
        month,
        day,
        hour,
        minute,
        second,
        micros: 0,
    }
}

fn check_attacks(root: &Path, smells: &BTreeMap<String, bool>) -> Result<()> {
    let doc = read_json(root, "attacks/sql.json")?;
    if bool_of(get(&doc, "runtime_executed")?)? {
        return Err(err("sql fixture claims a runtime pass"));
    }
    let text = fs::read_to_string(root.join("attacks/sql.json"))?;
    if text.contains("model_verdict") {
        return Err(err("sql fixture contains a model verdict"));
    }
    let mut categories = BTreeSet::new();
    let mut ids = BTreeMap::new();
    let mut reasons = BTreeMap::new();
    for item in arr(get(&doc, "cases")?)? {
        let id = str_of(get(item, "id")?)?.to_string();
        let category = str_of(get(item, "category")?)?.to_string();
        let decision = str_of(get(item, "expected_decision")?)?;
        let reason = str_of(get(item, "expected_reason")?)?;
        reasons.insert(id.clone(), reason.to_string());
        let legal = bool_of(get(item, "legal_control")?)?;
        if reason.is_empty() || str_of(get(item, "sql")?)?.is_empty() {
            return Err(err(format!("empty sql case {id}")));
        }
        let ok = if legal {
            decision == "allow" || decision == "expand_star"
        } else {
            decision == "deny"
        };
        if !ok {
            return Err(err(format!("sql decision drift on {id}")));
        }
        categories.insert(category);
        ids.insert(id, decision.to_string());
    }
    for category in CATEGORIES {
        if !categories.contains(*category) {
            return Err(err(format!("missing sql category {category}")));
        }
    }
    for (id, decision) in [
        ("SQL-STAR-XL", "expand_star"),
        ("SQL-R9-GJ", "deny"),
        ("SQL-LEGAL-EAST", "allow"),
        ("SQL-LEGAL-DISPATCH", "allow"),
        ("SQL-COUNT-GJ", "allow"),
        ("SQL-MASKED-NAME", "allow"),
        ("SQL-DENY-WEST", "deny"),
        ("SQL-DENY-MANAGER-WEST", "deny"),
        ("SQL-DENY-CK02", "deny"),
        ("SQL-RAW-SFZH", "deny"),
        ("SQL-CANCEL", "deny"),
        ("SQL-COMMENT", "deny"),
        ("SQL-WCTE", "deny"),
        ("SQL-PG-SLEEP", "deny"),
        ("SQL-SET-ROLE", "deny"),
        ("SQL-SET-CLAIMS", "deny"),
        ("SQL-CATALOG-PG", "deny"),
        ("SQL-SCHEMA-ESCAPE", "deny"),
        ("SQL-UNION", "deny"),
        ("SQL-LATERAL", "deny"),
        ("SQL-LOCK", "deny"),
        ("SQL-RECURSIVE", "deny"),
        ("SQL-COPY", "deny"),
        ("SQL-VIEW-SUBQUERY", "deny"),
        ("SQL-CARTESIAN", "deny"),
        ("SQL-LEGAL-JOIN", "allow"),
    ] {
        if ids.get(id).map(String::as_str) != Some(decision) {
            return Err(err(format!("sql case {id} decision drift")));
        }
    }
    for (id, reason) in [
        ("SQL-VIEW-SUBQUERY", "view_or_subquery_row_filter_bypass"),
        ("SQL-CARTESIAN", "cartesian_product"),
        ("SQL-LEGAL-JOIN", "bounded_equijoin"),
    ] {
        if reasons.get(id).map(String::as_str) != Some(reason) {
            return Err(err(format!("sql case {id} reason drift")));
        }
    }
    let prompt = read_json(root, "attacks/prompt-injection.json")?;
    eq(str_of(get(&prompt, "canary")?)?, PROMPT_CANARY, "canary")?;
    if bool_of(get(&prompt, "real_secret")?)? || !bool_of(get(&prompt, "in_untrusted_comment")?)? {
        return Err(err("prompt fixture drift"));
    }
    if bool_of(get(&prompt, "in_customer_note")?)? != flag(smells, "prompt_injection") {
        return Err(err("prompt note flag drift"));
    }
    Ok(())
}

fn check_authz(root: &Path) -> Result<()> {
    let roles = read_json(root, "authorization/roles.json")?;
    if bool_of(get(&roles, "runtime_executed")?)? {
        return Err(err("authz claims a runtime pass"));
    }
    let kh = load_csv(&root.join(catalog::KH.file))?;
    let mut by_name = BTreeMap::new();
    for row in &kh.rows {
        by_name.insert(req(&kh, row, "khmc")?.to_string(), row);
    }
    let mut seen = BTreeSet::new();
    for role in arr(get(&roles, "roles")?)? {
        let name = str_of(get(role, "role")?)?;
        seen.insert(name.to_string());
        if name == "dispatcher" {
            contains_str(get(role, "tables_allow")?, "yd")?;
            contains_str(get(role, "tables_deny")?, "sk")?;
            contains_str(get(role, "columns_deny")?, "sfzh")?;
        }
        if name == "finance_east" {
            let row = get(role, "kh_row")?;
            eq(str_of(get(row, "szdq")?)?, "east", "finance east region")?;
            eq(str_of(get(row, "csbz")?)?, "0", "finance east csbz")?;
            eq(str_of(get(row, "scbz")?)?, "*", "finance east scbz")?;
        }
        if name == "manager_east" {
            eq(
                str_of(get(get(role, "kh_row")?, "scbz")?)?,
                "0",
                "manager scbz",
            )?;
        }
        if name == "warehouse" {
            eq(str_of(get(role, "ckid")?)?, "CK01", "warehouse")?;
        }
        if name == "auditor" {
            contains_str(get(role, "columns_deny")?, "sfzh")?;
        }
    }
    for required in [
        "dispatcher",
        "finance_east",
        "finance_west",
        "manager_east",
        "warehouse",
        "auditor",
    ] {
        if !seen.contains(required) {
            return Err(err(format!("missing role {required}")));
        }
    }
    let decisions = read_json(root, "authorization/decisions.json")?;
    for item in arr(get(&decisions, "cases")?)? {
        let id = str_of(get(item, "id")?)?;
        let role = str_of(get(item, "role")?)?;
        let subject = str_of(get(item, "subject")?)?;
        let decision = str_of(get(item, "decision")?)?;
        if str_of(get(item, "kind")?)? == "customer" {
            let row = by_name
                .get(subject)
                .ok_or_else(|| err(format!("decision subject missing {subject}")))?;
            let allow = allows(
                role,
                req(&kh, row, "szdq")?,
                req(&kh, row, "csbz")?,
                req(&kh, row, "scbz")?,
            );
            let expect = if allow { "allow" } else { "deny" };
            if decision != expect {
                return Err(err(format!("authz decision drift on {id}")));
            }
        }
    }
    must_decision(&decisions, "AUTH-WH-CK01", "allow")?;
    must_decision(&decisions, "AUTH-WH-CK02", "deny")?;
    must_decision(&decisions, "AUTH-AUD-RAW", "deny")?;
    must_decision(&decisions, "AUTH-AUD-NAME", "allow")?;
    must_decision(&decisions, "AUTH-DISP-FIN", "deny")?;
    must_decision(&decisions, "AUTH-DISP-YD", "allow")?;
    let matrix = read_json(root, "authorization/matrix.json")?;
    let mut cells = BTreeSet::new();
    for cell in arr(get(&matrix, "cells")?)? {
        cells.insert(format!(
            "{}/{}/{}",
            str_of(get(cell, "role")?)?,
            str_of(get(cell, "table")?)?,
            str_of(get(cell, "decision")?)?
        ));
    }
    if !cells.contains("dispatcher/yd/allow") || !cells.contains("dispatcher/sk/deny") {
        return Err(err("authz matrix drift"));
    }
    let mut role_allow = BTreeMap::<String, BTreeSet<String>>::new();
    let mut role_deny = BTreeMap::<String, BTreeSet<String>>::new();
    for role in arr(get(&roles, "roles")?)? {
        let name = str_of(get(role, "role")?)?.to_string();
        let mut allow = BTreeSet::new();
        let mut deny = BTreeSet::new();
        for table in arr(get(role, "tables_allow")?)? {
            allow.insert(str_of(table)?.to_string());
        }
        for table in arr(get(role, "tables_deny")?)? {
            deny.insert(str_of(table)?.to_string());
        }
        if !allow.is_disjoint(&deny) {
            return Err(err(format!("role matrix overlap {name}")));
        }
        let mut union = allow.clone();
        union.extend(deny.iter().cloned());
        let expect: BTreeSet<String> = catalog::BUSINESS_TABLES
            .iter()
            .map(|table| (*table).to_string())
            .collect();
        if union != expect {
            return Err(err(format!("role matrix incomplete {name}")));
        }
        role_allow.insert(name.clone(), allow);
        role_deny.insert(name, deny);
    }
    if cells.len() != catalog::BUSINESS_TABLES.len() * role_allow.len() {
        return Err(err("authz matrix drift"));
    }
    for (role, allow) in &role_allow {
        for table in allow {
            if !cells.contains(&format!("{role}/{table}/allow")) {
                return Err(err(format!("authz matrix missing {role}/{table}")));
            }
        }
    }
    for (role, deny) in &role_deny {
        for table in deny {
            if !cells.contains(&format!("{role}/{table}/deny")) {
                return Err(err(format!("authz matrix missing {role}/{table}")));
            }
        }
    }
    if !cells.contains("dispatcher/zd/allow") || !cells.contains("dispatcher/kc/deny") {
        return Err(err("authz matrix drift"));
    }
    if !cells.contains("warehouse/zd/allow")
        || !cells.contains("warehouse/gj/deny")
        || !cells.contains("warehouse/kh_bm/deny")
        || !cells.contains("warehouse/cl/deny")
        || !cells.contains("warehouse/xl/deny")
    {
        return Err(err("authz matrix drift"));
    }
    Ok(())
}

fn allows(role: &str, szdq: &str, csbz: &str, scbz: &str) -> bool {
    match role {
        "finance_east" => szdq == "east" && csbz == "0",
        "finance_west" => szdq == "west" && csbz == "0",
        "manager_east" => szdq == "east" && csbz == "0" && scbz == "0",
        _ => false,
    }
}

fn must_decision(doc: &JsonNode, id: &str, decision: &str) -> Result<()> {
    for item in arr(get(doc, "cases")?)? {
        if str_of(get(item, "id")?)? == id {
            eq(str_of(get(item, "decision")?)?, decision, id)?;
            return Ok(());
        }
    }
    Err(err(format!("missing decision {id}")))
}

fn check_ddl(root: &Path, smells: &BTreeMap<String, bool>) -> Result<()> {
    let integer_fen = flag(smells, "cents");
    let overlap = flag(smells, "dst_overlap");
    let mysql = fs::read_to_string(root.join("source-inputs/mysql-erp/ddl.sql"))?;
    let pg = fs::read_to_string(root.join("source-inputs/postgres-tms/ddl.sql"))?;
    if ddl_has_foreign_key(&mysql) || ddl_has_foreign_key(&pg) {
        return Err(err("ddl contains a foreign key"));
    }
    let mysql_parsed = parse_creates(&mysql)?;
    let pg_parsed = parse_creates(&pg)?;
    let mut mysql_expect = Vec::new();
    for table in catalog::mysql_tables() {
        mysql_expect.push(expected_table(table, integer_fen));
    }
    diff_tables(&mysql_parsed, &mysql_expect)?;
    let mut pg_expect = Vec::new();
    for table in catalog::postgres_tables() {
        if table.name == "gj" {
            for shard in verify_shards(overlap) {
                pg_expect.push(ExpectedTable {
                    name: format!("gj_{shard}"),
                    columns: table
                        .columns
                        .iter()
                        .map(|column| ExpectedColumn {
                            name: column.name,
                            sql: catalog::effective_sql(table.dialect, column, integer_fen),
                            nullable: column.nullable,
                        })
                        .collect(),
                    pk: Vec::new(),
                });
            }
        } else {
            pg_expect.push(expected_table(table, integer_fen));
        }
    }
    diff_tables(&pg_parsed, &pg_expect)?;
    check_catalog_file(
        root,
        "source-inputs/mysql-erp/catalog.json",
        catalog::mysql_tables(),
        integer_fen,
        None,
    )?;
    let shard_names: Vec<String> = verify_shards(overlap)
        .into_iter()
        .map(|shard| format!("gj_{shard}"))
        .collect();
    check_catalog_file(
        root,
        "source-inputs/postgres-tms/catalog.json",
        catalog::postgres_tables(),
        integer_fen,
        Some(&shard_names),
    )?;
    Ok(())
}

struct ExpectedColumn {
    name: &'static str,
    sql: &'static str,
    nullable: bool,
}

struct ExpectedTable {
    name: String,
    columns: Vec<ExpectedColumn>,
    pk: Vec<&'static str>,
}

fn expected_table(table: &Table, integer_fen: bool) -> ExpectedTable {
    ExpectedTable {
        name: table.name.to_string(),
        columns: table
            .columns
            .iter()
            .map(|column| ExpectedColumn {
                name: column.name,
                sql: catalog::effective_sql(table.dialect, column, integer_fen),
                nullable: column.nullable,
            })
            .collect(),
        pk: table.pk.to_vec(),
    }
}

fn verify_shards(overlap: bool) -> Vec<u32> {
    let mut shards: Vec<u32> = (202601..=202609).collect();
    if overlap {
        shards.push(202611);
    }
    shards
}

fn diff_tables(parsed: &[crate::ddl::ParsedTable], expected: &[ExpectedTable]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for table in parsed {
        if !seen.insert(table.name.clone()) {
            return Err(err(format!("duplicate create table {}", table.name)));
        }
    }
    for table in expected {
        let got = parsed
            .iter()
            .find(|item| item.name == table.name)
            .ok_or_else(|| err(format!("ddl is missing table {}", table.name)))?;
        if got.pk.len() != table.pk.len()
            || got
                .pk
                .iter()
                .zip(table.pk.iter())
                .any(|(left, right)| left != right)
        {
            return Err(err(format!("primary key drift on {}", table.name)));
        }
        if got.columns.len() != table.columns.len() {
            return Err(err(format!("ddl column drift on {}", table.name)));
        }
        for (index, column) in table.columns.iter().enumerate() {
            let actual = &got.columns[index];
            if actual.name != column.name {
                return Err(err(format!(
                    "ddl column {}.{} is {}, expected {}",
                    table.name, column.name, actual.name, column.name
                )));
            }
            if actual.sql != column.sql {
                return Err(err(format!(
                    "ddl type {}.{} is {}, expected {}",
                    table.name, column.name, actual.sql, column.sql
                )));
            }
            if actual.nullable != column.nullable {
                return Err(err(format!(
                    "ddl nullability {}.{}",
                    table.name, column.name
                )));
            }
        }
    }
    for table in parsed {
        if !expected.iter().any(|item| item.name == table.name) {
            return Err(err(format!("unexpected create table {}", table.name)));
        }
    }
    Ok(())
}

fn check_catalog_file(
    root: &Path,
    rel: &str,
    tables: &[&Table],
    integer_fen: bool,
    shards: Option<&[String]>,
) -> Result<()> {
    let doc = read_json(root, rel)?;
    let listed = arr(get(&doc, "tables")?)?;
    if listed.len() != tables.len() {
        return Err(err(format!("catalog table count drift in {rel}")));
    }
    for (table, node) in tables.iter().zip(listed.iter()) {
        let name = str_of(get(node, "name")?)?;
        if name != table.name {
            return Err(err(format!("catalog table {name} expected {}", table.name)));
        }
        let pk = arr(get(node, "pk")?)?;
        if pk.len() != table.pk.len() {
            return Err(err(format!("primary key drift on {name}")));
        }
        for (got, expect) in pk.iter().zip(table.pk.iter()) {
            if str_of(got)? != *expect {
                return Err(err(format!("primary key drift on {name}")));
            }
        }
        let columns = arr(get(node, "columns")?)?;
        if columns.len() != table.columns.len() {
            return Err(err(format!("catalog column drift on {name}")));
        }
        for (column, node) in table.columns.iter().zip(columns.iter()) {
            let got_name = str_of(get(node, "name")?)?;
            if got_name != column.name {
                return Err(err(format!(
                    "catalog column {name}.{got_name} expected {}",
                    column.name
                )));
            }
            let expect_sql = catalog::effective_sql(table.dialect, column, integer_fen);
            let got_sql = str_of(get(node, "sql")?)?;
            if got_sql != expect_sql {
                return Err(err(format!(
                    "catalog type {name}.{} is {got_sql}, expected {expect_sql}",
                    column.name
                )));
            }
            if bool_of(get(node, "nullable")?)? != column.nullable {
                return Err(err(format!("catalog nullability {name}.{}", column.name)));
            }
        }
        if table.name == "gj" {
            let got = arr(get(node, "shards")?)?;
            let expect = shards.unwrap_or(&[]);
            if got.len() != expect.len() {
                return Err(err("catalog shard list drift"));
            }
            for (item, name) in got.iter().zip(expect.iter()) {
                if str_of(item)? != name {
                    return Err(err("catalog shard list drift"));
                }
            }
        }
    }
    Ok(())
}

fn check_metrics(root: &Path, smells: &BTreeMap<String, bool>) -> Result<()> {
    let integer_fen = flag(smells, "cents");
    let split = flag(smells, "metric_invoice_vs_receipt");
    let closing = flag(smells, "closing_period");
    let doc = read_json(root, "hidden-truth/metrics.json")?;
    let present = obj(&doc)?
        .iter()
        .any(|(key, _)| key == "amount_representation");
    if integer_fen {
        if present {
            return Err(err("amount representation drift"));
        }
    } else {
        eq(
            str_of(get(&doc, "amount_representation")?)?,
            "fixed_point_yuan_scale_2",
            "amount representation",
        )?;
    }
    let march = if split { 7_550 } else { 17_550 };
    let april = if split { 10_000 } else { 0 };
    let closing_march = if split && !closing { 7_550 } else { 17_550 };
    let jzbz = if split && closing { 1 } else { 0 };
    metric_eq(get(&doc, "invoice_fen")?, HX_INVOICE_FEN, integer_fen)?;
    metric_eq(get(&doc, "order_fen")?, HX_ORDER_FEN, integer_fen)?;
    metric_eq(get(&doc, "calendar_march_receipt_fen")?, march, integer_fen)?;
    metric_eq(get(&doc, "closing_april_receipt_fen")?, april, integer_fen)?;
    metric_eq(
        get(&doc, "closing_march_receipt_fen")?,
        closing_march,
        integer_fen,
    )?;
    if u64_of(get(&doc, "closing_april_jzbz")?)? != jzbz {
        return Err(err("metrics closing_april_jzbz drift"));
    }
    let rule = if closing {
        "Calendar March is skrq in 2026-03. March closing also takes jzbz=1 receipts dated 2026-04-01 through 2026-04-05."
    } else {
        "Calendar March is skrq in 2026-03. Closing follows the receipt month and does not take April receipts."
    };
    eq(str_of(get(&doc, "rule")?)?, rule, "metrics rule")?;
    Ok(())
}

fn metric_eq(node: &JsonNode, cents: i64, integer_fen: bool) -> Result<()> {
    let got = if integer_fen {
        match node {
            JsonNode::String(_) => return Err(err("amount is not integer fen")),
            _ => i64::try_from(u64_of(node)?).map_err(|_| err("amount is not integer fen"))?,
        }
    } else {
        match node {
            JsonNode::String(text) => parse_money(text, false)?,
            _ => {
                return Err(err(
                    "amount is not exact fixed-point yuan while cents is off",
                ));
            }
        }
    };
    if got != cents {
        return Err(err("metrics amount drift"));
    }
    Ok(())
}

fn check_anomalies(root: &Path, smells: &BTreeMap<String, bool>) -> Result<()> {
    let doc = read_json(root, "hidden-truth/anomalies.json")?;
    let mut seen = BTreeSet::new();
    for item in arr(get(&doc, "smells")?)? {
        let name = str_of(get(item, "smell")?)?;
        let enabled = bool_of(get(item, "enabled")?)?;
        if !SMELLS.contains(&name) {
            return Err(err(format!("unknown anomaly {name}")));
        }
        if enabled != flag(smells, name) {
            return Err(err(format!("anomaly flag drift on {name}")));
        }
        seen.insert(name.to_string());
    }
    for name in SMELLS {
        if !seen.contains(*name) {
            return Err(err(format!("anomaly missing {name}")));
        }
    }
    Ok(())
}

fn check_encoding(root: &Path, smells: &BTreeMap<String, bool>) -> Result<()> {
    let gbk = fs::read(root.join("source-inputs/encoding/legacy-gbk.bin"))?;
    let rare = fs::read(root.join("source-inputs/encoding/legacy-gb18030.bin"))?;
    if flag(smells, "gbk_bytes") {
        if gbk != gbk_hx() || std::str::from_utf8(&gbk).is_ok() {
            return Err(err("gbk fixture drift"));
        }
    } else if !gbk.is_empty() {
        return Err(err("gbk bytes present while the smell is off"));
    }
    if flag(smells, "gb18030_bytes") {
        if rare != gb18030_rare() || std::str::from_utf8(&rare).is_ok() {
            return Err(err("gb18030 fixture drift"));
        }
    } else if !rare.is_empty() {
        return Err(err("gb18030 bytes present while the smell is off"));
    }
    let meta = read_json(root, "source-inputs/encoding/metadata.json")?;
    for item in arr(get(&meta, "files")?)? {
        let name = str_of(get(item, "name")?)?;
        let included = bool_of(get(item, "included")?)?;
        if name == "legacy-gbk.bin" && included != flag(smells, "gbk_bytes") {
            return Err(err("gbk metadata drift"));
        }
        if name == "legacy-gb18030.bin" && included != flag(smells, "gb18030_bytes") {
            return Err(err("gb18030 metadata drift"));
        }
        let hex = str_of(get(item, "hex")?)?;
        if !hex.is_ascii() {
            return Err(err("encoding metadata is not ascii hex"));
        }
    }
    Ok(())
}

fn check_dirty(root: &Path) -> Result<()> {
    let invalid = load_csv(&root.join("source-inputs/mysql-erp/dirty/invalid-enum.csv"))?;
    let mut nope = false;
    for row in &invalid.rows {
        if req(&invalid, row, "khzt_enum")? == "nope" {
            nope = true;
        }
    }
    if !nope {
        return Err(err("invalid enum fixture missing"));
    }
    let dup = load_csv(&root.join("source-inputs/mysql-erp/dirty/duplicate-pk.csv"))?;
    let mut keys = BTreeMap::<String, u64>::new();
    for row in &dup.rows {
        *keys.entry(req(&dup, row, "khid")?.to_string()).or_insert(0) += 1;
    }
    if !keys.values().any(|count| *count >= 2) {
        return Err(err("duplicate pk fixture missing"));
    }
    let nul = fs::read(root.join("source-inputs/mysql-erp/dirty/nul-byte.bin"))?;
    if !nul.contains(&0) {
        return Err(err("nul fixture missing"));
    }
    let load = fs::read_to_string(root.join("source-inputs/mysql-erp/load.json"))?;
    if load.contains("dirty/") {
        return Err(err("load manifest includes a dirty file"));
    }
    Ok(())
}

fn partition_of(kh: &Sheet) -> Result<u16> {
    let row = kh
        .rows
        .first()
        .ok_or_else(|| err("customer table is empty"))?;
    let id = req(kh, row, "khid")?;
    if id.len() < 5 || !id.starts_with('K') {
        return Err(err("customer id shape"));
    }
    u16::from_str_radix(&id[1..5], 16).map_err(|_| err("customer id partition"))
}

fn check_ids(sheet: &Sheet, column: &str, prefix: char, part: u16) -> Result<()> {
    let expect = format!("{prefix}{part:04X}");
    for row in &sheet.rows {
        if !req(sheet, row, column)?.starts_with(&expect) {
            return Err(err("id partition drift"));
        }
    }
    Ok(())
}

fn count_csv(path: &Path) -> Result<(Vec<String>, u64)> {
    let file = File::open(path).map_err(|_| err(format!("missing {}", path.display())))?;
    let scan = for_each_row(file, |_| Ok(()))?;
    Ok((scan.header, scan.rows))
}

fn headers_match(got: &[String], expect: &[&str]) -> bool {
    got.len() == expect.len()
        && got
            .iter()
            .zip(expect.iter())
            .all(|(left, right)| left == *right)
}

fn load_csv(path: &Path) -> Result<Sheet> {
    let file = File::open(path).map_err(|_| err(format!("missing {}", path.display())))?;
    let mut rows = Vec::new();
    let scan = for_each_row(file, |row| {
        rows.push(row.to_vec());
        Ok(())
    })?;
    Ok(Sheet {
        header: scan.header,
        rows,
    })
}

fn require_unique(sheet: &Sheet, pk: &[&str]) -> Result<()> {
    let mut indexes = Vec::new();
    for name in pk {
        indexes.push(col(sheet, name)?);
    }
    let mut seen = BTreeSet::new();
    for row in &sheet.rows {
        let mut key = String::new();
        for index in &indexes {
            key.push('\u{1f}');
            key.push_str(req_at(row, *index)?);
        }
        if !seen.insert(key) {
            return Err(err(format!("duplicate primary key {}", pk.join(","))));
        }
    }
    Ok(())
}

fn col(sheet: &Sheet, name: &str) -> Result<usize> {
    sheet
        .header
        .iter()
        .position(|item| item == name)
        .ok_or_else(|| err(format!("missing column {name}")))
}

fn req<'a>(sheet: &'a Sheet, row: &'a [Option<String>], name: &str) -> Result<&'a str> {
    let index = col(sheet, name)?;
    row.get(index)
        .and_then(|cell| cell.as_deref())
        .ok_or_else(|| err(format!("null {name}")))
}

fn req_at(row: &[Option<String>], index: usize) -> Result<&str> {
    row.get(index)
        .and_then(|cell| cell.as_deref())
        .ok_or_else(|| err("null primary key"))
}

fn opt<'a>(sheet: &'a Sheet, row: &'a [Option<String>], name: &str) -> Result<Option<&'a str>> {
    let index = col(sheet, name)?;
    Ok(row.get(index).and_then(|cell| cell.as_deref()))
}

fn flag(smells: &BTreeMap<String, bool>, name: &str) -> bool {
    smells.get(name).copied().unwrap_or(false)
}

fn read_json(root: &Path, rel: &str) -> Result<JsonNode> {
    let text = fs::read_to_string(root.join(rel)).map_err(|_| err(format!("missing {rel}")))?;
    parse_json(text.trim_end()).map_err(|_| err(format!("json rejected {rel}")))
}

fn get<'a>(node: &'a JsonNode, key: &str) -> Result<&'a JsonNode> {
    obj(node)?
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
        .ok_or_else(|| err(format!("missing {key}")))
}

fn obj(node: &JsonNode) -> Result<&[(String, JsonNode)]> {
    match node {
        JsonNode::Object(pairs) => Ok(pairs),
        _ => Err(err("expected object")),
    }
}

fn arr(node: &JsonNode) -> Result<&[JsonNode]> {
    match node {
        JsonNode::Array(items) => Ok(items),
        _ => Err(err("expected array")),
    }
}

fn str_of(node: &JsonNode) -> Result<&str> {
    match node {
        JsonNode::String(value) => Ok(value),
        _ => Err(err("expected string")),
    }
}

fn bool_of(node: &JsonNode) -> Result<bool> {
    match node {
        JsonNode::Bool(value) => Ok(*value),
        _ => Err(err("expected bool")),
    }
}

fn u64_of(node: &JsonNode) -> Result<u64> {
    match node {
        JsonNode::Number(value) => {
            if value.is_negative() || value.scale() != 0 {
                return Err(err("expected integer"));
            }
            value
                .coefficient()
                .parse()
                .map_err(|_| err("integer overflow"))
        }
        _ => Err(err("expected integer")),
    }
}

fn lookup_u64(pairs: &[(String, JsonNode)], key: &str) -> Result<u64> {
    u64_of(lookup_node(pairs, key)?)
}

fn lookup_node<'a>(pairs: &'a [(String, JsonNode)], key: &str) -> Result<&'a JsonNode> {
    pairs
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
        .ok_or_else(|| err(format!("missing {key}")))
}

fn contains_str(node: &JsonNode, expect: &str) -> Result<()> {
    for item in arr(node)? {
        if str_of(item)? == expect {
            return Ok(());
        }
    }
    Err(err(format!("missing {expect}")))
}

fn eq(actual: &str, expect: &str, label: &str) -> Result<()> {
    if actual == expect {
        Ok(())
    } else {
        Err(err(format!("{label} drift")))
    }
}
