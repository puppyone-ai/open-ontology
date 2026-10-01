//! Streaming dataset writer. Bytes hit the stage directory and are promoted
//! only after the manifest sidecar is in place.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use oo_core::tz::TZDB_VERSION;
use oo_core::{CANONICAL_VALUE_VERSION, TYPE_POLICY_V1};

use crate::attacks::{prompt_json, sql_json};
use crate::authz::{decisions_json, matrix_json, role_names, roles_json};
use crate::catalog::{self, Col, GJ, TXN, Table, header};
use crate::csvfile::{CsvOut, Field, mysql_string};
use crate::ddl::{mysql_ddl, postgres_ddl_for};
use crate::digest::{HashWriter, sha256_hex, tree_preimage};
use crate::encoding::{gb18030_rare, gbk_hx};
use crate::error::{Result, err};
use crate::facts::{
    self, AS_OF, AUTHZ_POLICY, BUSINESS_TZ, DATASET, PROMPT_CANARY, PROTOCOL, SMELLS,
};
use crate::jsonutil::{J, arr, b, obj, s, u};
use crate::pathsafety::{
    Exists, create_new_file, create_stage, placement, promote, remove_owned_stage, validate_rel,
};
use crate::plan::{Counts, Scale, SmellSet, active_shards, counts_profile, shard_list, shard_rows};
use crate::prng::{PRNG_VERSION, domain_partition};
use crate::rows::{self, Side};
use crate::truth::{self, TruthIn};
use crate::verify::verify_dataset;

const VIS_SOURCE: &str = "source-input";
const VIS_TRUTH: &str = "hidden-truth";
const VIS_ATTACK: &str = "attack";
const VIS_AUTH: &str = "authorization";

const LEAK: &[&str] = &[
    facts::TRUTH_MARKER,
    "do_not_merge",
    "hidden-truth",
    "expected_decision",
    "gold_sql",
    "entity_cluster",
    "oo-datagen-stage",
];

pub struct Options {
    pub scale: Scale,
    pub seed: u64,
    pub output: PathBuf,
    pub scope: PathBuf,
    pub exists: Exists,
    pub smells: SmellSet,
    pub stop_after_files: Option<u32>,
}

#[derive(Debug)]
pub struct Report {
    pub tree_sha256: String,
    pub scale: String,
    pub seed: u64,
    pub partition: u16,
}

struct FileRec {
    bytes: u64,
    sha256: String,
    visibility: &'static str,
}

struct Sink {
    root: PathBuf,
    files: BTreeMap<String, FileRec>,
    opened: u32,
    stop: Option<u32>,
}

impl Sink {
    fn new(root: PathBuf, stop: Option<u32>) -> Self {
        Self {
            root,
            files: BTreeMap::new(),
            opened: 0,
            stop,
        }
    }

    fn gate(&self) -> Result<()> {
        if let Some(stop) = self.stop
            && self.opened >= stop
        {
            return Err(err("injected stop"));
        }
        Ok(())
    }

    fn write_bytes(&mut self, rel: &str, bytes: &[u8], visibility: &'static str) -> Result<()> {
        let file = self.start(rel, visibility)?;
        let mut writer = HashWriter::new(BufWriter::new(file));
        writer.write_all(bytes)?;
        let (buf, len, digest) = writer.finish()?;
        finish_buf(buf, rel, len, digest, visibility, self)
    }

    fn write_text(&mut self, rel: &str, text: &str, visibility: &'static str) -> Result<()> {
        if text.contains('\r') {
            return Err(err(format!("{rel} contains CR")));
        }
        self.write_bytes(rel, text.as_bytes(), visibility)
    }

    fn write_json(&mut self, rel: &str, value: &J, visibility: &'static str) -> Result<()> {
        let text = value.render()?;
        self.write_text(rel, &text, visibility)
    }

    fn begin_csv(
        &mut self,
        rel: &str,
        header: &[&str],
        visibility: &'static str,
    ) -> Result<CsvOut<BufWriter<File>>> {
        let file = self.start(rel, visibility)?;
        CsvOut::new(BufWriter::new(file), header)
    }

    fn finish_csv(
        &mut self,
        rel: &str,
        visibility: &'static str,
        csv: CsvOut<BufWriter<File>>,
    ) -> Result<()> {
        let (buf, len, digest) = csv.finish()?;
        finish_buf(buf, rel, len, digest, visibility, self)
    }

    fn start(&mut self, rel: &str, visibility: &'static str) -> Result<File> {
        self.gate()?;
        validate_rel(rel)?;
        if vis_of(rel)? != visibility {
            return Err(err(format!("visibility does not match {rel}")));
        }
        if self.files.contains_key(rel) {
            return Err(err(format!("duplicate dataset path {rel}")));
        }
        let path = self.root.join(rel);
        let file = create_new_file(&path)?;
        self.opened += 1;
        Ok(file)
    }
}

fn finish_buf(
    mut buf: BufWriter<File>,
    rel: &str,
    len: u64,
    digest: String,
    visibility: &'static str,
    sink: &mut Sink,
) -> Result<()> {
    buf.flush()?;
    let file = buf.into_inner().map_err(|error| err(error.to_string()))?;
    file.sync_all()?;
    sink.files.insert(
        rel.to_string(),
        FileRec {
            bytes: len,
            sha256: digest,
            visibility,
        },
    );
    Ok(())
}

fn vis_of(rel: &str) -> Result<&'static str> {
    if rel.starts_with("source-inputs/") {
        Ok(VIS_SOURCE)
    } else if rel.starts_with("hidden-truth/") {
        Ok(VIS_TRUTH)
    } else if rel.starts_with("attacks/") {
        Ok(VIS_ATTACK)
    } else if rel.starts_with("authorization/") {
        Ok(VIS_AUTH)
    } else {
        Err(err(format!("no visibility for {rel}")))
    }
}

pub fn generate(options: &Options) -> Result<Report> {
    if !options.scale.executed() {
        return Err(err(format!(
            "scale {} is reserved for SP12 and is not executed",
            options.scale.as_str()
        )));
    }
    let place = placement(&options.scope, &options.output)?;
    if fs::symlink_metadata(&place.dest).is_ok() {
        return match options.exists {
            Exists::Check => verify_dataset(&place.requested),
            Exists::Fail | Exists::Resume => Err(err("destination exists")),
        };
    }
    if options.exists == Exists::Check {
        return Err(err("check requires an existing dataset"));
    }
    if fs::symlink_metadata(&place.stage).is_ok() || fs::symlink_metadata(&place.mark).is_ok() {
        if options.exists != Exists::Resume {
            return Err(err("stage already exists"));
        }
        remove_owned_stage(&place.stage, &place.mark)?;
    }
    create_stage(&place.stage, &place.mark)?;
    match generate_into(&place.stage, options) {
        Ok(report) => {
            promote(&place.stage, &place.mark, &place.dest)?;
            Ok(report)
        }
        Err(error) => {
            let _ = remove_owned_stage(&place.stage, &place.mark);
            Err(error)
        }
    }
}

fn generate_into(stage: &Path, options: &Options) -> Result<Report> {
    let overlap = options.smells.enabled("dst_overlap");
    let counts = counts_profile(options.scale, &options.smells);
    let part = domain_partition(options.seed);
    let shards = active_shards(overlap);
    let mut sink = Sink::new(stage.to_path_buf(), options.stop_after_files);
    write_sources(&mut sink, options, &counts, part, &shards)?;
    let side = write_tables(&mut sink, options, &counts, part, &shards)?;
    let shard_counts = shard_count_list(&counts, overlap);
    write_truth_files(&mut sink, options, &counts, part, &side, &shard_counts)?;
    write_attacks(&mut sink, &options.smells)?;
    write_authz(&mut sink, &options.smells)?;
    reject_source_leaks(stage)?;
    let tree = seal(&mut sink, options, &counts, part)?;
    let dir = File::open(stage)?;
    dir.sync_all()?;
    Ok(Report {
        tree_sha256: tree,
        scale: options.scale.as_str().to_string(),
        seed: options.seed,
        partition: part,
    })
}

fn shard_count_list(counts: &Counts, overlap: bool) -> Vec<(u32, u64)> {
    shard_list()
        .iter()
        .map(|shard| {
            let rows = if *shard == 202611 && !overlap {
                0
            } else {
                shard_rows(counts.track, *shard, overlap)
            };
            (*shard, rows)
        })
        .collect()
}

fn write_sources(
    sink: &mut Sink,
    options: &Options,
    counts: &Counts,
    part: u16,
    shards: &[u32],
) -> Result<()> {
    let integer_fen = options.smells.enabled("cents");
    sink.write_text("source-inputs/README.txt", SOURCE_README, VIS_SOURCE)?;
    sink.write_text(
        "source-inputs/mysql-erp/ddl.sql",
        &mysql_ddl(integer_fen),
        VIS_SOURCE,
    )?;
    sink.write_text(
        "source-inputs/postgres-tms/ddl.sql",
        &postgres_ddl_for(shards, integer_fen),
        VIS_SOURCE,
    )?;
    sink.write_json(
        "source-inputs/mysql-erp/catalog.json",
        &catalog_doc(
            "mysql-erp",
            "mysql",
            catalog::mysql_tables(),
            None,
            integer_fen,
        ),
        VIS_SOURCE,
    )?;
    sink.write_json(
        "source-inputs/postgres-tms/catalog.json",
        &catalog_doc(
            "postgres-tms",
            "postgres",
            catalog::postgres_tables(),
            Some(shards),
            integer_fen,
        ),
        VIS_SOURCE,
    )?;
    sink.write_json(
        "source-inputs/mysql-erp/load.json",
        &load_doc("mysql", "erp", mysql_load()),
        VIS_SOURCE,
    )?;
    sink.write_json(
        "source-inputs/postgres-tms/load.json",
        &load_doc("postgres", "tms", pg_load(shards)),
        VIS_SOURCE,
    )?;
    let sample = mysql_string("a'b\\c\n");
    let untrusted = format!(
        "-- UNTRUSTED source comment. Not applied.\n-- {PROMPT_CANARY}\n-- quoted {sample}\nSELECT 1;\n"
    );
    sink.write_text(
        "source-inputs/comments/untrusted.sql",
        &untrusted,
        VIS_SOURCE,
    )?;
    sink.write_json(
        "source-inputs/comments/provenance.json",
        &obj(vec![
            ("applied", b(false)),
            ("trust", s("untrusted")),
            (
                "reason",
                s("source comments are not schema and are not business truth"),
            ),
        ]),
        VIS_SOURCE,
    )?;
    write_encoding(sink, &options.smells)?;
    write_dirty(sink)?;
    sink.write_json(
        "source-inputs/workload/load.json",
        &load_doc("csv", "workload", vec![load_item(&TXN)]),
        VIS_SOURCE,
    )?;
    sink.write_json(
        "source-inputs/workload/month-end.json",
        &month_end_doc(counts, part),
        VIS_SOURCE,
    )?;
    let script = if counts.month_end == 0 {
        format!(
            "-- synthetic month-end script. Not executed against a database.\n-- month_end_txn is off. No ME batch is written.\n-- scale {scale} is the only scale this file was generated for.\n",
            scale = options.scale.as_str()
        )
    } else {
        format!(
            "-- synthetic month-end script. Not executed against a database.\n-- epoch 1 txn ME{part:04X}0000000 updates ys.\n-- scale {scale} is the only scale this file was generated for.\n",
            scale = options.scale.as_str()
        )
    };
    sink.write_text("source-inputs/workload/month-end.sql", &script, VIS_SOURCE)?;
    Ok(())
}

const SOURCE_README: &str = "\
OO-Logistics source inputs.\n\
Dialect extracts, untrusted comments, and dirty side files.\n\
Null in CSV is the two-character sequence backslash N.\n\
Text encoding is UTF-8 except the binary files named by encoding/metadata.json.\n\
This directory does not state merge decisions, official totals, or role grants.\n";

fn write_encoding(sink: &mut Sink, smells: &SmellSet) -> Result<()> {
    let gbk = if smells.enabled("gbk_bytes") {
        gbk_hx()
    } else {
        Vec::new()
    };
    let rare = if smells.enabled("gb18030_bytes") {
        gb18030_rare()
    } else {
        Vec::new()
    };
    sink.write_bytes("source-inputs/encoding/legacy-gbk.bin", &gbk, VIS_SOURCE)?;
    sink.write_bytes(
        "source-inputs/encoding/legacy-gb18030.bin",
        &rare,
        VIS_SOURCE,
    )?;
    let meta = obj(vec![
        (
            "files",
            arr(vec![
                encoding_item("legacy-gbk.bin", "GBK", smells.enabled("gbk_bytes"), &gbk),
                encoding_item(
                    "legacy-gb18030.bin",
                    "GB18030",
                    smells.enabled("gb18030_bytes"),
                    &rare,
                ),
            ]),
        ),
        (
            "note",
            s(
                "Binary fixtures are not UTF-8 and are not embedded in JSON. The hex field is ASCII metadata.",
            ),
        ),
    ]);
    sink.write_json("source-inputs/encoding/metadata.json", &meta, VIS_SOURCE)
}

fn encoding_item(name: &str, encoding: &str, included: bool, bytes: &[u8]) -> J {
    obj(vec![
        ("encoding", s(encoding)),
        ("hex", s(hex_encode(bytes))),
        ("included", b(included)),
        ("name", s(name)),
        ("sha256", s(sha256_hex(bytes))),
    ])
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn write_dirty(sink: &mut Sink) -> Result<()> {
    let readme = "\
These side files are not listed in load.json.\n\
invalid-enum.csv holds a value outside the enum.\n\
duplicate-pk.csv repeats a key.\n\
nul-byte.bin holds a zero byte and is not CSV.\n";
    sink.write_text(
        "source-inputs/mysql-erp/dirty/README.txt",
        readme,
        VIS_SOURCE,
    )?;
    let mut invalid = sink.begin_csv(
        "source-inputs/mysql-erp/dirty/invalid-enum.csv",
        &["khid", "khzt_enum"],
        VIS_SOURCE,
    )?;
    invalid.write_row(&[Field::Raw("BADENUM"), Field::Raw("nope")])?;
    sink.finish_csv(
        "source-inputs/mysql-erp/dirty/invalid-enum.csv",
        VIS_SOURCE,
        invalid,
    )?;
    let mut dup = sink.begin_csv(
        "source-inputs/mysql-erp/dirty/duplicate-pk.csv",
        &["khid", "khmc"],
        VIS_SOURCE,
    )?;
    dup.write_row(&[Field::Raw("DUPKEY"), Field::Text("一")])?;
    dup.write_row(&[Field::Raw("DUPKEY"), Field::Text("二")])?;
    sink.finish_csv(
        "source-inputs/mysql-erp/dirty/duplicate-pk.csv",
        VIS_SOURCE,
        dup,
    )?;
    sink.write_bytes(
        "source-inputs/mysql-erp/dirty/nul-byte.bin",
        b"NUL\0",
        VIS_SOURCE,
    )
}

fn write_tables(
    sink: &mut Sink,
    options: &Options,
    counts: &Counts,
    part: u16,
    shards: &[u32],
) -> Result<Side> {
    let mut kh = open_table(sink, &catalog::KH)?;
    let side = rows::write_customers(&mut kh, counts, options.seed, part, &options.smells)?;
    sink.finish_csv(catalog::KH.file, VIS_SOURCE, kh)?;
    let mut alias = open_table(sink, &catalog::KH_BM)?;
    rows::write_aliases(&mut alias, counts, part, &options.smells)?;
    sink.finish_csv(catalog::KH_BM.file, VIS_SOURCE, alias)?;
    let mut ht = open_table(sink, &catalog::HT)?;
    rows::write_contracts(&mut ht, counts, part, &options.smells)?;
    sink.finish_csv(catalog::HT.file, VIS_SOURCE, ht)?;
    let mut dd = open_table(sink, &catalog::DD)?;
    let mut ys = open_table(sink, &catalog::YS)?;
    let mut sk = open_table(sink, &catalog::SK)?;
    let mut tk = open_table(sink, &catalog::TK)?;
    let mut fp = open_table(sink, &catalog::FP)?;
    let mut yd = open_table(sink, &catalog::YD)?;
    let mut mx = open_table(sink, &catalog::YD_MX)?;
    rows::write_orders(
        &mut rows::OrderOut {
            dd: &mut dd,
            ys: &mut ys,
            sk: &mut sk,
            tk: &mut tk,
            fp: &mut fp,
            yd: &mut yd,
            mx: &mut mx,
        },
        counts,
        part,
        &options.smells,
    )?;
    sink.finish_csv(catalog::DD.file, VIS_SOURCE, dd)?;
    sink.finish_csv(catalog::YS.file, VIS_SOURCE, ys)?;
    sink.finish_csv(catalog::SK.file, VIS_SOURCE, sk)?;
    sink.finish_csv(catalog::TK.file, VIS_SOURCE, tk)?;
    sink.finish_csv(catalog::FP.file, VIS_SOURCE, fp)?;
    sink.finish_csv(catalog::YD.file, VIS_SOURCE, yd)?;
    sink.finish_csv(catalog::YD_MX.file, VIS_SOURCE, mx)?;
    let mut zd = open_table(sink, &catalog::ZD)?;
    let dict_n = rows::write_dict(&mut zd, &options.smells)?;
    if dict_n != counts.dict {
        return Err(err(format!("dict rows {dict_n} != {}", counts.dict)));
    }
    sink.finish_csv(catalog::ZD.file, VIS_SOURCE, zd)?;
    let mut yh = open_table(sink, &catalog::YH)?;
    rows::write_users(&mut yh, part)?;
    sink.finish_csv(catalog::YH.file, VIS_SOURCE, yh)?;
    let mut cl = open_table(sink, &catalog::CL)?;
    let mut sj = open_table(sink, &catalog::SJ)?;
    let mut xl = open_table(sink, &catalog::XL)?;
    let mut ck = open_table(sink, &catalog::CK)?;
    let mut kc = open_table(sink, &catalog::KC)?;
    rows::write_fleet(
        &mut rows::FleetOut {
            cl: &mut cl,
            sj: &mut sj,
            xl: &mut xl,
            ck: &mut ck,
            kc: &mut kc,
        },
        counts,
        part,
        &options.smells,
    )?;
    sink.finish_csv(catalog::CL.file, VIS_SOURCE, cl)?;
    sink.finish_csv(catalog::SJ.file, VIS_SOURCE, sj)?;
    sink.finish_csv(catalog::XL.file, VIS_SOURCE, xl)?;
    sink.finish_csv(catalog::CK.file, VIS_SOURCE, ck)?;
    sink.finish_csv(catalog::KC.file, VIS_SOURCE, kc)?;
    let gj_header = header(&GJ);
    let mut gj = BTreeMap::new();
    for shard in shards {
        let rel = format!("source-inputs/postgres-tms/shards/gj_{shard}.csv");
        gj.insert(*shard, sink.begin_csv(&rel, &gj_header, VIS_SOURCE)?);
    }
    rows::write_tracks(&mut gj, counts, options.seed, part, &options.smells)?;
    for (shard, csv) in gj {
        let rel = format!("source-inputs/postgres-tms/shards/gj_{shard}.csv");
        sink.finish_csv(&rel, VIS_SOURCE, csv)?;
    }
    let mut txn = open_table(sink, &TXN)?;
    rows::write_txn(&mut txn, counts, part, &options.smells)?;
    sink.finish_csv(TXN.file, VIS_SOURCE, txn)?;
    Ok(side)
}

fn open_table(sink: &mut Sink, table: &Table) -> Result<CsvOut<BufWriter<File>>> {
    let columns = header(table);
    sink.begin_csv(table.file, &columns, VIS_SOURCE)
}

fn write_truth_files(
    sink: &mut Sink,
    options: &Options,
    counts: &Counts,
    part: u16,
    side: &Side,
    shards: &[(u32, u64)],
) -> Result<()> {
    let input = TruthIn {
        scale: options.scale.as_str(),
        seed: options.seed,
        part,
        counts,
        smells: &options.smells,
        side,
        shards,
        tzdb: TZDB_VERSION,
    };
    sink.write_text("hidden-truth/README.txt", &truth::readme(), VIS_TRUTH)?;
    sink.write_json(
        "hidden-truth/semantics.json",
        &truth::semantics(&options.smells),
        VIS_TRUTH,
    )?;
    sink.write_json(
        "hidden-truth/relationships.json",
        &truth::relationships(&input),
        VIS_TRUTH,
    )?;
    sink.write_json(
        "hidden-truth/enums.json",
        &truth::enums_doc(&input),
        VIS_TRUTH,
    )?;
    sink.write_json(
        "hidden-truth/entities.json",
        &truth::entities(&input),
        VIS_TRUTH,
    )?;
    sink.write_json(
        "hidden-truth/metrics.json",
        &truth::metrics(&input),
        VIS_TRUTH,
    )?;
    sink.write_json(
        "hidden-truth/calendar.json",
        &truth::calendar(&options.smells),
        VIS_TRUTH,
    )?;
    sink.write_json(
        "hidden-truth/expected-counts.json",
        &truth::expected_counts(&input),
        VIS_TRUTH,
    )?;
    sink.write_json(
        "hidden-truth/anomalies.json",
        &truth::anomalies(&input),
        VIS_TRUTH,
    )?;
    sink.write_json(
        "hidden-truth/invariants.json",
        &truth::invariants(&options.smells),
        VIS_TRUTH,
    )?;
    sink.write_json("hidden-truth/facts.json", &truth::facts(&input), VIS_TRUTH)
}

fn write_attacks(sink: &mut Sink, smells: &SmellSet) -> Result<()> {
    sink.write_json("attacks/sql.json", &sql_json()?, VIS_ATTACK)?;
    sink.write_json(
        "attacks/prompt-injection.json",
        &prompt_json(smells.enabled("prompt_injection")),
        VIS_ATTACK,
    )
}

fn write_authz(sink: &mut Sink, smells: &SmellSet) -> Result<()> {
    sink.write_json("authorization/roles.json", &roles_json(), VIS_AUTH)?;
    sink.write_json("authorization/matrix.json", &matrix_json(), VIS_AUTH)?;
    sink.write_json(
        "authorization/decisions.json",
        &decisions_json(smells),
        VIS_AUTH,
    )
}

fn reject_source_leaks(stage: &Path) -> Result<()> {
    let root = stage.join("source-inputs");
    let mut files = Vec::new();
    walk_files(&root, &mut files)?;
    for path in files {
        let rel = path
            .strip_prefix(stage)
            .map_err(|_| err("stage walk escaped"))?;
        let rel = rel
            .to_str()
            .ok_or_else(|| err("source path is not utf-8"))?;
        if rel.ends_with(".bin") {
            continue;
        }
        let bytes = fs::read(&path)?;
        if bytes.contains(&0) {
            return Err(err(format!("{rel} contains NUL")));
        }
        let text = String::from_utf8(bytes).map_err(|_| err(format!("{rel} is not utf-8")))?;
        if text.contains('\r') {
            return Err(err(format!("{rel} contains CR")));
        }
        for token in LEAK {
            if text.contains(token) {
                return Err(err(format!("{rel} leaks {token}")));
            }
        }
    }
    Ok(())
}

fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let meta = fs::symlink_metadata(entry.path())?;
        if meta.file_type().is_symlink() {
            return Err(err("refusing to walk a symlink"));
        }
        if meta.is_dir() {
            walk_files(&entry.path(), out)?;
        } else if meta.is_file() {
            out.push(entry.path());
        }
    }
    Ok(())
}

fn seal(sink: &mut Sink, options: &Options, counts: &Counts, part: u16) -> Result<String> {
    let entries: Vec<(String, u64, String)> = sink
        .files
        .iter()
        .map(|(path, rec)| (path.clone(), rec.bytes, rec.sha256.clone()))
        .collect();
    let tree = tree_preimage(&entries);
    let files = sink
        .files
        .iter()
        .map(|(path, rec)| {
            obj(vec![
                ("bytes", u(rec.bytes)),
                ("path", s(path)),
                ("sha256", s(&rec.sha256)),
                ("visibility", s(rec.visibility)),
            ])
        })
        .collect();
    let manifest = obj(vec![
        ("artifact_owner", s("bench/datagen")),
        ("as_of", s(AS_OF)),
        ("authz_policy", s(AUTHZ_POLICY)),
        ("business_tz", s(BUSINESS_TZ)),
        ("canonical_value", s(CANONICAL_VALUE_VERSION)),
        ("consumer_default", s("source-inputs")),
        ("counts", counts_json(counts)),
        ("dataset", s(DATASET)),
        ("encoding", s("utf-8")),
        ("files", arr(files)),
        ("generator_version", s(env!("CARGO_PKG_VERSION"))),
        ("newline", s("lf")),
        ("partition", s(format!("{part:04X}"))),
        ("prng", s(PRNG_VERSION)),
        ("product_pack", b(false)),
        ("protocol", s(PROTOCOL)),
        ("roles", arr(role_names().into_iter().map(s).collect())),
        (
            "roots",
            arr(vec![
                root_item("source-inputs", VIS_SOURCE),
                root_item("hidden-truth", VIS_TRUTH),
                root_item("attacks", VIS_ATTACK),
                root_item("authorization", VIS_AUTH),
            ]),
        ),
        ("scale", s(options.scale.as_str())),
        ("seed", s(options.seed.to_string())),
        ("smells", smell_json(&options.smells)),
        (
            "sp12",
            obj(vec![
                ("executed", b(false)),
                ("l_track_rows", u(Scale::L.track_rows())),
                ("m_track_rows", u(Scale::M.track_rows())),
            ]),
        ),
        ("tree_sha256", s(&tree)),
        ("type_policy", s(TYPE_POLICY_V1)),
        ("tzdb", s(TZDB_VERSION)),
    ]);
    let bytes = manifest.render()?.into_bytes();
    write_untracked(sink, "dataset-manifest.json", &bytes)?;
    let sidecar = format!("{}\n", sha256_hex(&bytes));
    write_untracked(sink, "dataset-manifest.sha256", sidecar.as_bytes())?;
    Ok(tree)
}

fn write_untracked(sink: &mut Sink, rel: &str, bytes: &[u8]) -> Result<()> {
    sink.gate()?;
    validate_rel(rel)?;
    let path = sink.root.join(rel);
    let mut file = create_new_file(&path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    sink.opened += 1;
    Ok(())
}

fn counts_json(counts: &Counts) -> J {
    obj(vec![
        ("aliases", u(counts.aliases)),
        ("contract_rows", u(counts.contract_rows)),
        ("contracts", u(counts.contracts)),
        ("customers", u(counts.customers)),
        ("dict", u(counts.dict)),
        ("drivers", u(counts.drivers)),
        ("inventory", u(counts.inventory)),
        ("invoices", u(counts.invoices)),
        ("lines", u(counts.lines)),
        ("month_end", u(counts.month_end)),
        ("orders", u(counts.orders)),
        ("receipts", u(counts.receipts)),
        ("receivables", u(counts.receivables)),
        ("refunds", u(counts.refunds)),
        ("routes", u(counts.routes)),
        ("skus", u(counts.skus)),
        ("track", u(counts.track)),
        ("txn_rows", u(counts.txn_rows)),
        ("users", u(counts.users)),
        ("vehicles", u(counts.vehicles)),
        ("warehouses", u(counts.warehouses)),
        ("waybills", u(counts.waybills)),
    ])
}

fn smell_json(smells: &SmellSet) -> J {
    obj(SMELLS
        .iter()
        .map(|name| (*name, b(smells.enabled(name))))
        .collect())
}

fn root_item(path: &str, visibility: &str) -> J {
    obj(vec![("path", s(path)), ("visibility", s(visibility))])
}

fn catalog_doc(
    source: &str,
    dialect: &str,
    tables: &[&Table],
    shards: Option<&[u32]>,
    integer_fen: bool,
) -> J {
    let items = tables
        .iter()
        .map(|table| {
            let shard_names = if table.name == "gj" { shards } else { None };
            table_json(table, shard_names, dialect, integer_fen)
        })
        .collect();
    obj(vec![
        ("dialect", s(dialect)),
        ("source", s(source)),
        ("tables", arr(items)),
    ])
}

fn table_json(table: &Table, shards: Option<&[u32]>, dialect: &str, integer_fen: bool) -> J {
    let mut pairs = vec![
        (
            "columns",
            arr(table
                .columns
                .iter()
                .map(|column| col_json(column, dialect, integer_fen))
                .collect()),
        ),
        ("name", s(table.name)),
        ("pk", arr(table.pk.iter().copied().map(s).collect())),
    ];
    if let Some(shards) = shards {
        pairs.push((
            "shards",
            arr(shards
                .iter()
                .map(|shard| s(format!("gj_{shard}")))
                .collect()),
        ));
    }
    obj(pairs)
}

fn col_json(col: &Col, dialect: &str, integer_fen: bool) -> J {
    obj(vec![
        ("name", s(col.name)),
        ("nullable", b(col.nullable)),
        (
            "sql",
            s(catalog::effective_sql(dialect, col, integer_fen)),
        ),
    ])
}

fn load_doc(dialect: &str, database: &str, tables: Vec<J>) -> J {
    obj(vec![
        ("database", s(database)),
        ("database_executed", b(false)),
        ("dialect", s(dialect)),
        ("encoding", s("utf-8")),
        ("header", b(true)),
        ("newline", s("lf")),
        ("null", s("\\N")),
        ("tables", arr(tables)),
    ])
}

fn mysql_load() -> Vec<J> {
    catalog::mysql_tables()
        .iter()
        .map(|table| load_item(table))
        .collect()
}

fn pg_load(shards: &[u32]) -> Vec<J> {
    let mut items = Vec::new();
    for table in catalog::postgres_tables() {
        if table.name == "gj" {
            for shard in shards {
                items.push(obj(vec![
                    (
                        "file",
                        s(format!("source-inputs/postgres-tms/shards/gj_{shard}.csv")),
                    ),
                    ("pk", arr(vec![])),
                    ("table", s(format!("gj_{shard}"))),
                ]));
            }
        } else {
            items.push(load_item(table));
        }
    }
    items
}

fn load_item(table: &Table) -> J {
    obj(vec![
        ("file", s(table.file)),
        ("pk", arr(table.pk.iter().copied().map(s).collect())),
        ("table", s(table.name)),
    ])
}

fn month_end_doc(counts: &Counts, part: u16) -> J {
    obj(vec![
        ("as_of", s(AS_OF)),
        ("database_executed", b(false)),
        ("epoch", u(1)),
        ("log_materialized", b(true)),
        ("op", s("update")),
        ("rel", s("ys")),
        ("rows", u(counts.month_end)),
        (
            "sp12_targets",
            obj(vec![
                ("executed", b(false)),
                ("l_track_rows", u(Scale::L.track_rows())),
                ("m_track_rows", u(Scale::M.track_rows())),
            ]),
        ),
        ("txn_id", s(format!("ME{part:04X}0000000"))),
    ])
}
