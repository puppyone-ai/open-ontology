//! Independent review of the frozen role-pack-bundle command.
//! Inputs are the kiln documents next to this file. Hashes are computed here.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use std::os::unix::fs::MetadataExt;

const CLERK_REQUEST: &str = include_str!("kiln-clerk.json");
const AUDIT_REQUEST: &str = include_str!("kiln-audit.json");
const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const STATUSES: &str = "{\"authentication\":\"not_claimed\",\"authorization\":\"not_claimed\",\"domains\":\"unsupported\",\"file_splitting\":\"unsupported\",\"mount\":\"not_performed\",\"publication\":\"not_claimed\",\"row_security_reproven\":false,\"samples\":\"omitted\",\"search\":\"unsupported\",\"tokens_estimator\":\"none\",\"tokens_status\":\"unsupported\"}";
const USAGE: &str = "role-pack-bundle: oo.builder.role-pack-bundle/v1/usage arguments are --request, --output, or --help";
const DESTINATION: &str = "role-pack-bundle: oo.builder.role-pack-bundle/v1/io.destination the output path already exists";
const COLLISION: &str = "role-pack-bundle: oo.builder.role-pack-bundle/v1/io.collision the staging path already exists";
const UNSUPPORTED: &str = "role-pack-bundle: oo.builder.role-pack-bundle/v1/io.link the filesystem does not support a hard link";
const LINK_OTHER: &str = "role-pack-bundle: oo.builder.role-pack-bundle/v1/io.link the staging file could not be linked";

fn main() {
    if let Err(err) = run() {
        eprintln!("review_oracle: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    self_check()?;
    let mut args = env::args().skip(1);
    let Some(compiler) = args.next() else {
        println!("self_check=pass");
        return Ok(());
    };
    let bundle = args.next().ok_or("usage: review_oracle <oo-builder> <role-pack-bundle> <work> <report> <probe>")?;
    let work = args.next().ok_or("missing work dir")?;
    let report = args.next().ok_or("missing report")?;
    let probe = args.next().ok_or("missing probe dir")?;
    if args.next().is_some() {
        return Err("unexpected argument".to_owned());
    }
    let compiler = PathBuf::from(compiler);
    let bundle = PathBuf::from(bundle);
    let work = PathBuf::from(work);
    let report = PathBuf::from(report);
    let probe = PathBuf::from(probe);
    let mut lines = Vec::new();
    match review(&compiler, &bundle, &work, &probe, &mut lines) {
        Ok(()) => {
            lines.push("review_result=pass".to_owned());
            flush(&report, &lines)?;
            println!("review_result=pass");
            Ok(())
        }
        Err(err) => {
            lines.push(format!("review_result=fail"));
            lines.push(format!("fail={err}"));
            let _ = flush(&report, &lines);
            Err(err)
        }
    }
}

fn review(
    compiler: &Path,
    bundle: &Path,
    work: &Path,
    probe: &Path,
    lines: &mut Vec<String>,
) -> Result<(), String> {
    fs::create_dir_all(work).map_err(|err| format!("work dir {}", kind_name(err.kind())))?;
    let tmp_probe = probe_hard_link(&work.join("probe-tmp"))?;
    lines.push(format!("hardlink_tmpfs={tmp_probe}"));
    let bind_probe = probe_hard_link(probe)?;
    lines.push(format!("hardlink_bind={bind_probe}"));
    let tmp_supported = tmp_probe.starts_with("supported");

    let clerk_cmd = compile_role(compiler, CLERK_REQUEST.as_bytes())?;
    let audit_cmd = compile_role(compiler, AUDIT_REQUEST.as_bytes())?;
    lines.push(format!(
        "compiler_has_cache_key={} compiler_has_clerk_principal={} compiler_has_audit_principal={}",
        clerk_cmd.contains("cache_key") && audit_cmd.contains("cache_key"),
        clerk_cmd.contains("prin-clerk-a238"),
        audit_cmd.contains("prin-audit-a238")
    ));
    if !clerk_cmd.contains("cache_key") || !audit_cmd.contains("cache_key") {
        return Err("compiler result dropped cache_key".to_owned());
    }
    if !clerk_cmd.contains("prin-clerk-a238") || !audit_cmd.contains("prin-audit-a238") {
        return Err("compiler result dropped the caller principal".to_owned());
    }

    let clerk_req = work.join("clerk-request.json");
    let audit_req = work.join("audit-request.json");
    fs::write(&clerk_req, CLERK_REQUEST).map_err(|_| "write clerk request".to_owned())?;
    fs::write(&audit_req, AUDIT_REQUEST).map_err(|_| "write audit request".to_owned())?;
    let clerk_out = work.join("clerk-bundle.json");
    let audit_out = work.join("audit-bundle.json");
    if !tmp_supported {
        let refused = run_bundle(bundle, &clerk_req, &clerk_out)?;
        expect_exact(&refused, 1, UNSUPPORTED)?;
        if clerk_out.exists() {
            return Err("unsupported tmpfs still created the output".to_owned());
        }
        lines.push("product_on_tmpfs=unsupported_sentence".to_owned());
        lines.push("supported_publish=not_run".to_owned());
        return Ok(());
    }
    let clerk_run = run_bundle(bundle, &clerk_req, &clerk_out)?;
    let audit_run = run_bundle(bundle, &audit_req, &audit_out)?;
    expect_success(&clerk_run)?;
    expect_success(&audit_run)?;
    lines.push("product_on_tmpfs=published".to_owned());
    if staging_of(&clerk_out).exists() || staging_of(&audit_out).exists() {
        return Err("staging name remained after publish".to_owned());
    }
    let clerk_bytes = fs::read(&clerk_out).map_err(|_| "read clerk bundle".to_owned())?;
    let audit_bytes = fs::read(&audit_out).map_err(|_| "read audit bundle".to_owned())?;
    check_bundle("clerk", &clerk_bytes, &clerk_cmd, "oo_r_kiln_clerk", lines)?;
    check_bundle("audit", &audit_bytes, &audit_cmd, "oo_r_kiln_audit", lines)?;
    if lines.iter().any(|line| line.starts_with("clerk_pack_sha256="))
        && lines.iter().any(|line| line.starts_with("audit_pack_sha256="))
    {
        let clerk_sha = value_of(lines, "clerk_pack_sha256");
        let audit_sha = value_of(lines, "audit_pack_sha256");
        lines.push(format!("pack_sha256_differ={}", clerk_sha != audit_sha));
        if clerk_sha == audit_sha {
            return Err("the two roles produced one pack_sha256".to_owned());
        }
    }
    role_split(&clerk_bytes, &audit_bytes)?;

    sentinel(bundle, &clerk_req, &work.join("sentinel"))?;
    lines.push("sentinel=unchanged".to_owned());
    symlink_dest(bundle, &clerk_req, &work.join("symlink"))?;
    lines.push("symlink_destination=unchanged".to_owned());
    directory_dest(bundle, &clerk_req, &work.join("directory"))?;
    lines.push("directory_destination=unchanged".to_owned());
    preexisting_staging(bundle, &clerk_req, &work.join("staging"))?;
    lines.push("preexisting_staging=unchanged".to_owned());

    concurrent_same(bundle, &clerk_req, &audit_req, &clerk_bytes, &audit_bytes, &work.join("same"))?;
    lines.push("concurrent_same=one_winner_foreign_unchanged".to_owned());
    concurrent_distinct(bundle, &clerk_req, &audit_req, &clerk_bytes, &audit_bytes, &work.join("distinct"))?;
    lines.push("concurrent_distinct=own_outputs_foreign_unchanged".to_owned());

    help_and_usage(bundle, &work.join("help"))?;
    lines.push("help=no_auth_mount_publish_no_file".to_owned());

    if bind_probe.starts_with("supported") {
        lines.push("product_unsupported=not_mounted".to_owned());
    } else {
        let bind_result = product_on_probe(bundle, &clerk_req, probe, &bind_probe)?;
        lines.push(format!("product_unsupported={bind_result}"));
    }

    let window = observe_windows(bundle, &clerk_req, &work.join("window"))?;
    lines.push(format!("link_window={window}"));
    Ok(())
}

fn check_bundle(
    label: &str,
    bytes: &[u8],
    command: &str,
    role: &str,
    lines: &mut Vec<String>,
) -> Result<(), String> {
    if bytes.ends_with(b"\n") {
        return Err(format!("{label} bundle has a trailing newline"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| format!("{label} bundle is not utf-8"))?;
    for marker in ["prin-clerk-a238", "prin-audit-a238", "cache_key", "principal"] {
        if text.contains(marker) {
            return Err(format!("{label} bundle contains {marker}"));
        }
    }
    let pack = field_raw(text, "pack")?;
    let manifest = field_raw(text, "manifest")?;
    let command_pack = field_raw(command, "artifact")?;
    let command_manifest = field_raw(command, "manifest")?;
    if pack != command_pack {
        return Err(format!("{label} pack bytes differ from the compiler artifact"));
    }
    if manifest != command_manifest {
        return Err(format!("{label} manifest bytes differ from the compiler manifest"));
    }
    let pack_sha = hex(&sha256(pack.as_bytes()));
    let manifest_sha = hex(&sha256(manifest.as_bytes()));
    let file_sha = hex(&sha256(bytes));
    if string_field(text, "pack_sha256")? != pack_sha {
        return Err(format!("{label} pack_sha256 does not match the spliced pack"));
    }
    if string_field(text, "manifest_sha256")? != manifest_sha {
        return Err(format!("{label} manifest_sha256 does not match the spliced manifest"));
    }
    if string_field(command, "manifest_sha256")? != manifest_sha {
        return Err(format!("{label} command manifest_sha256 differs"));
    }
    if string_field(&pack, "manifest_sha256")? != manifest_sha {
        return Err(format!("{label} pack manifest_sha256 differs"));
    }
    let digest = string_field(text, "pack_digest")?;
    if digest != string_field(command, "pack_digest")? || digest != string_field(&pack, "pack_digest")? {
        return Err(format!("{label} pack_digest was not copied"));
    }
    if digest == pack_sha {
        return Err(format!("{label} pack_digest equals pack_sha256"));
    }
    let pack_len: usize = field_raw(text, "pack_length")?.parse().map_err(|_| "pack_length".to_owned())?;
    let manifest_len: usize = field_raw(text, "manifest_length")?
        .parse()
        .map_err(|_| "manifest_length".to_owned())?;
    if pack_len != pack.len() || manifest_len != manifest.len() {
        return Err(format!("{label} length field differs from the spliced bytes"));
    }
    if string_field(text, "protocol")? != "oo.builder.role-pack-bundle/v1" {
        return Err(format!("{label} protocol"));
    }
    if string_field(text, "source_protocol")? != "oo.builder.role-pack-command/v1" {
        return Err(format!("{label} source_protocol"));
    }
    if string_field(text, "pack_protocol")? != "oo.builder.role-pack/v1" {
        return Err(format!("{label} pack_protocol"));
    }
    if string_field(text, "compiler")? != "oo-builder/role-pack/v1" {
        return Err(format!("{label} compiler"));
    }
    if field_raw(text, "statuses")? != STATUSES {
        return Err(format!("{label} statuses were not the fixed table"));
    }
    let snapshot = field_raw(text, "snapshot")?;
    let expected = expected_snapshot(&manifest, role)?;
    if snapshot != expected {
        return Err(format!("{label} snapshot does not match the manifest identities"));
    }
    let keys = object_keys(text)?;
    let expect_keys = [
        "compiler",
        "manifest",
        "manifest_length",
        "manifest_sha256",
        "pack",
        "pack_digest",
        "pack_length",
        "pack_protocol",
        "pack_sha256",
        "protocol",
        "snapshot",
        "source_protocol",
        "statuses",
    ];
    if keys != expect_keys {
        return Err(format!("{label} bundle keys differ"));
    }
    lines.push(format!("{label}_file_sha256={file_sha}"));
    lines.push(format!("{label}_pack_sha256={pack_sha}"));
    lines.push(format!("{label}_manifest_sha256={manifest_sha}"));
    lines.push(format!("{label}_pack_digest={digest}"));
    lines.push(format!("{label}_pack_length={pack_len}"));
    lines.push(format!("{label}_manifest_length={manifest_len}"));
    lines.push(format!("{label}_snapshot={snapshot}"));
    Ok(())
}

fn expected_snapshot(manifest: &str, role: &str) -> Result<String, String> {
    let project = string_field(manifest, "project")?;
    let manifest_role = string_field(manifest, "role")?;
    let ontology = string_field(manifest, "ontology_version")?;
    let catalog = string_field(manifest, "catalog_version")?;
    let policy = field_raw(manifest, "policy")?;
    let knowledge = field_raw(manifest, "knowledge")?;
    let policy_id = string_field(&policy, "id")?;
    let policy_version = string_field(&policy, "version")?;
    let knowledge_id = string_field(&knowledge, "id")?;
    let knowledge_version = string_field(&knowledge, "version")?;
    if project != "kiln" || manifest_role != role || ontology != "ontology-a238" || catalog != "catalog-a238"
    {
        return Err(format!("manifest identity project={project} role={manifest_role}"));
    }
    if policy_id != "pol-kiln" || policy_version != "9" || knowledge_id != "know-kiln" || knowledge_version != "4" {
        return Err("manifest policy or knowledge identity".to_owned());
    }
    Ok(format!(
        "{{\"catalog_version\":{catalog},\"knowledge_id\":{knowledge_id},\"knowledge_version\":{knowledge_version},\"ontology_version\":{ontology},\"policy_id\":{policy_id},\"policy_version\":{policy_version},\"project\":{project},\"role\":{manifest_role}}}",
        catalog = json_string(&catalog),
        knowledge_id = json_string(&knowledge_id),
        knowledge_version = json_string(&knowledge_version),
        ontology = json_string(&ontology),
        policy_id = json_string(&policy_id),
        policy_version = json_string(&policy_version),
        project = json_string(&project),
        manifest_role = json_string(&manifest_role),
    ))
}

fn role_split(clerk: &[u8], audit: &[u8]) -> Result<(), String> {
    let clerk = std::str::from_utf8(clerk).map_err(|_| "clerk utf8".to_owned())?;
    let audit = std::str::from_utf8(audit).map_err(|_| "audit utf8".to_owned())?;
    for marker in [
        "tile-mark-a238",
        "kiln.tile",
        "one row per tile",
        "Estimated row count is omitted.",
        "has no fixed tokenizer",
        "not_claimed",
    ] {
        if !clerk.contains(marker) {
            return Err(format!("clerk bundle missing {marker}"));
        }
    }
    for marker in [
        "hearth-mark-a238",
        "kiln.hearth",
        "yard.pad",
        "pad-mark-a238",
        "vault_mark",
        "515151",
        "181818",
        "owner-a238-do-not-copy",
        "hidden_mass",
        "reading_c",
    ] {
        if clerk.contains(marker) {
            return Err(format!("clerk bundle contains {marker}"));
        }
    }
    for marker in ["kiln.hearth", "hearth-mark-a238", "estimated rows: 41", "not_claimed"] {
        if !audit.contains(marker) {
            return Err(format!("audit bundle missing {marker}"));
        }
    }
    for marker in [
        "tile-mark-a238",
        "kiln.tile",
        "yard.pad",
        "pad-mark-a238",
        "vault_mark",
        "515151",
        "181818",
        "owner-a238-do-not-copy",
        "stamp note stays",
    ] {
        if audit.contains(marker) {
            return Err(format!("audit bundle contains {marker}"));
        }
    }
    Ok(())
}

fn sentinel(bundle: &Path, request: &Path, dir: &Path) -> Result<(), String> {
    fresh_dir(dir)?;
    let dest = dir.join("out.json");
    let neighbor = dir.join("neighbor.txt");
    fs::write(&dest, b"SENTINEL-a238-keep").map_err(|_| "sentinel write".to_owned())?;
    fs::write(&neighbor, b"KEEP-NEIGHBOR-a238").map_err(|_| "neighbor write".to_owned())?;
    let output = run_bundle(bundle, request, &dest)?;
    expect_exact(&output, 1, DESTINATION)?;
    if fs::read(&dest).map_err(|_| "reread sentinel".to_owned())? != b"SENTINEL-a238-keep" {
        return Err("sentinel bytes changed".to_owned());
    }
    if fs::read(&neighbor).map_err(|_| "reread neighbor".to_owned())? != b"KEEP-NEIGHBOR-a238" {
        return Err("sentinel neighbor changed".to_owned());
    }
    if staging_of(&dest).exists() {
        return Err("sentinel staging remained".to_owned());
    }
    Ok(())
}

fn symlink_dest(bundle: &Path, request: &Path, dir: &Path) -> Result<(), String> {
    fresh_dir(dir)?;
    let real = dir.join("real.txt");
    let dest = dir.join("out.json");
    fs::write(&real, b"KEEP-REAL-a238").map_err(|_| "real write".to_owned())?;
    std::os::unix::fs::symlink(&real, &dest).map_err(|_| "symlink".to_owned())?;
    let output = run_bundle(bundle, request, &dest)?;
    expect_exact(&output, 1, DESTINATION)?;
    let meta = fs::symlink_metadata(&dest).map_err(|_| "symlink meta".to_owned())?;
    if !meta.file_type().is_symlink() {
        return Err("symlink destination was replaced".to_owned());
    }
    if fs::read(&real).map_err(|_| "reread real".to_owned())? != b"KEEP-REAL-a238" {
        return Err("symlink target changed".to_owned());
    }
    if staging_of(&dest).exists() {
        return Err("symlink staging remained".to_owned());
    }
    Ok(())
}

fn directory_dest(bundle: &Path, request: &Path, dir: &Path) -> Result<(), String> {
    fresh_dir(dir)?;
    let dest = dir.join("out.json");
    fs::create_dir(&dest).map_err(|_| "dest dir".to_owned())?;
    fs::write(dest.join("keep.txt"), b"KEEP-DIR-a238").map_err(|_| "dir child".to_owned())?;
    let output = run_bundle(bundle, request, &dest)?;
    expect_exact(&output, 1, DESTINATION)?;
    if !fs::symlink_metadata(&dest).map_err(|_| "dir meta".to_owned())?.is_dir() {
        return Err("directory destination was replaced".to_owned());
    }
    if fs::read(dest.join("keep.txt")).map_err(|_| "dir child read".to_owned())? != b"KEEP-DIR-a238" {
        return Err("directory child changed".to_owned());
    }
    if staging_of(&dest).exists() {
        return Err("directory staging remained".to_owned());
    }
    Ok(())
}

fn preexisting_staging(bundle: &Path, request: &Path, dir: &Path) -> Result<(), String> {
    fresh_dir(dir)?;
    let dest = dir.join("out.json");
    let staging = staging_of(&dest);
    fs::write(&staging, b"STAGING-KEEP-a238").map_err(|_| "staging write".to_owned())?;
    let output = run_bundle(bundle, request, &dest)?;
    expect_exact(&output, 1, COLLISION)?;
    if dest.exists() {
        return Err("preexisting staging still published".to_owned());
    }
    if fs::read(&staging).map_err(|_| "reread staging".to_owned())? != b"STAGING-KEEP-a238" {
        return Err("preexisting staging changed".to_owned());
    }
    Ok(())
}

fn concurrent_same(
    bundle: &Path,
    clerk_req: &Path,
    audit_req: &Path,
    clerk_bytes: &[u8],
    audit_bytes: &[u8],
    dir: &Path,
) -> Result<(), String> {
    fresh_dir(dir)?;
    let dest = dir.join("shared.json");
    let foreign = dir.join("FOREIGN-a238");
    fs::write(&foreign, b"KEEP-FOREIGN-a238").map_err(|_| "foreign".to_owned())?;
    let left = spawn_bundle(bundle, clerk_req, &dest)?;
    let right = spawn_bundle(bundle, audit_req, &dest)?;
    let left = left.wait_with_output().map_err(|_| "wait left".to_owned())?;
    let right = right.wait_with_output().map_err(|_| "wait right".to_owned())?;
    let wins = [left.status.success(), right.status.success()]
        .into_iter()
        .filter(|ok| *ok)
        .count();
    if wins != 1 {
        return Err(format!("concurrent same dest wins={wins}"));
    }
    let winner = fs::read(&dest).map_err(|_| "read shared".to_owned())?;
    if winner != clerk_bytes && winner != audit_bytes {
        return Err("concurrent shared bytes match neither role".to_owned());
    }
    let loser = if left.status.success() { &right } else { &left };
    if loser.status.code() != Some(1) {
        return Err("concurrent loser exit".to_owned());
    }
    let stderr = std::str::from_utf8(&loser.stderr).map_err(|_| "loser utf8".to_owned())?;
    if stderr != format!("{DESTINATION}\n") && stderr != format!("{COLLISION}\n") {
        return Err(format!("concurrent loser stderr {}", show_stderr(&loser.stderr)));
    }
    if !loser.stdout.is_empty() {
        return Err("concurrent loser stdout".to_owned());
    }
    if fs::read(&foreign).map_err(|_| "foreign read".to_owned())? != b"KEEP-FOREIGN-a238" {
        return Err("concurrent same overwrote the foreign file".to_owned());
    }
    if staging_of(&dest).exists() {
        return Err("concurrent staging remained".to_owned());
    }
    Ok(())
}

fn concurrent_distinct(
    bundle: &Path,
    clerk_req: &Path,
    audit_req: &Path,
    clerk_bytes: &[u8],
    audit_bytes: &[u8],
    dir: &Path,
) -> Result<(), String> {
    fresh_dir(dir)?;
    let clerk_out = dir.join("clerk.json");
    let audit_out = dir.join("audit.json");
    let foreign = dir.join("FOREIGN-a238");
    fs::write(&foreign, b"KEEP-FOREIGN-a238").map_err(|_| "foreign".to_owned())?;
    let left = spawn_bundle(bundle, clerk_req, &clerk_out)?;
    let right = spawn_bundle(bundle, audit_req, &audit_out)?;
    let left = left.wait_with_output().map_err(|_| "wait clerk".to_owned())?;
    let right = right.wait_with_output().map_err(|_| "wait audit".to_owned())?;
    expect_success(&left)?;
    expect_success(&right)?;
    if fs::read(&clerk_out).map_err(|_| "distinct clerk".to_owned())? != clerk_bytes {
        return Err("concurrent clerk output changed".to_owned());
    }
    if fs::read(&audit_out).map_err(|_| "distinct audit".to_owned())? != audit_bytes {
        return Err("concurrent audit output changed".to_owned());
    }
    if fs::read(&foreign).map_err(|_| "distinct foreign".to_owned())? != b"KEEP-FOREIGN-a238" {
        return Err("concurrent distinct overwrote the foreign file".to_owned());
    }
    if staging_of(&clerk_out).exists() || staging_of(&audit_out).exists() {
        return Err("distinct staging remained".to_owned());
    }
    Ok(())
}

fn help_and_usage(bundle: &Path, dir: &Path) -> Result<(), String> {
    fresh_dir(dir)?;
    let help = run_args(bundle, &["--help".to_owned()], None)?;
    if help.status.code() != Some(0) || !help.stderr.is_empty() {
        return Err("help exit".to_owned());
    }
    let text = String::from_utf8(help.stdout).map_err(|_| "help utf8".to_owned())?;
    for phrase in [
        "does not authenticate, grant, mount, tokenize, trust a cache, or publish",
        "Publication is a same-directory hard link.",
        "An existing output path is not replaced.",
        "A filesystem that does not support hard links fails publication.",
        "not a jail",
    ] {
        if !text.contains(phrase) {
            return Err(format!("help missing a required phrase"));
        }
    }
    if text.contains("rename") {
        return Err("help still names rename".to_owned());
    }
    if fs::read_dir(dir).map_err(|_| "help dir".to_owned())?.next().is_some() {
        return Err("help created a file".to_owned());
    }
    let usage = run_args(
        bundle,
        &["--help".to_owned(), "--request".to_owned(), "marker-a238-do-not-echo".to_owned()],
        None,
    )?;
    expect_exact(&usage, 2, USAGE)?;
    if String::from_utf8_lossy(&usage.stderr).contains("marker-a238-do-not-echo") {
        return Err("usage echoed the argument".to_owned());
    }
    Ok(())
}

fn product_on_probe(bundle: &Path, request: &Path, probe: &Path, probe_line: &str) -> Result<String, String> {
    let dir = probe.join("product");
    fresh_dir(&dir)?;
    let dest = dir.join("out.json");
    let output = run_bundle(bundle, request, &dest)?;
    let staging_left = staging_of(&dest).exists();
    let created = dest.exists();
    let stderr = std::str::from_utf8(&output.stderr).unwrap_or("");
    let class = if stderr == format!("{UNSUPPORTED}\n") {
        "unsupported_sentence"
    } else if stderr == format!("{LINK_OTHER}\n") {
        "other_link_sentence"
    } else {
        "other"
    };
    if created {
        let _ = fs::remove_file(&dest);
        return Err("unsupported probe created the output".to_owned());
    }
    if staging_left {
        return Err("unsupported probe left the staging file".to_owned());
    }
    let _ = fs::remove_dir_all(&dir);
    if !probe_line.contains("raw=1") && !probe_line.contains("kind=unsupported") && class == "unsupported_sentence" {
        return Err("product used the unsupported sentence for a different link error".to_owned());
    }
    Ok(format!("{class} exit={}", output.status.code().unwrap_or(-1)))
}

fn observe_windows(bundle: &Path, request: &Path, dir: &Path) -> Result<String, String> {
    let mut missed = 0u32;
    for index in 0..4 {
        let attempt = dir.join(format!("try-{index}"));
        fresh_dir(&attempt)?;
        let dest = attempt.join("out.json");
        let staging = staging_of(&dest);
        let stop = Arc::new(AtomicBool::new(false));
        let slot = Arc::new(Mutex::new(None));
        let stop_flag = Arc::clone(&stop);
        let slot_flag = Arc::clone(&slot);
        let dest_watch = dest.clone();
        let staging_watch = staging.clone();
        let watcher = thread::spawn(move || {
            while !stop_flag.load(Ordering::Relaxed) {
                if dest_watch.exists() && staging_watch.exists() {
                    if let (Ok(dest_meta), Ok(staging_meta)) = (
                        fs::symlink_metadata(&dest_watch),
                        fs::symlink_metadata(&staging_watch),
                    ) {
                        *slot_flag.lock().unwrap_or_else(|err| err.into_inner()) =
                            Some((dest_meta.ino(), staging_meta.ino(), dest_meta.nlink()));
                        break;
                    }
                }
                thread::yield_now();
            }
        });
        let output = run_bundle(bundle, request, &dest)?;
        stop.store(true, Ordering::Relaxed);
        let _ = watcher.join();
        expect_success(&output)?;
        let seen = slot.lock().unwrap_or_else(|err| err.into_inner()).clone();
        if let Some((dest_ino, staging_ino, nlink)) = seen {
            if dest_ino != staging_ino {
                return Err(format!("link window inodes differ nlink={nlink}"));
            }
            return Ok(format!("observed ino_eq=yes nlink={nlink} tries={}", index + 1));
        }
        missed += 1;
    }
    Ok(format!("not_observed tries={missed}"))
}

fn compile_role(compiler: &Path, request: &[u8]) -> Result<String, String> {
    let output = run_args(compiler, &["role-pack".to_owned()], Some(request))?;
    if output.status.code() != Some(0) || !output.stderr.is_empty() {
        return Err(format!(
            "role-pack exit={} stderr={}",
            output.status.code().unwrap_or(-1),
            show_stderr(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| "role-pack stdout is not utf-8".to_owned())
}

fn run_bundle(bundle: &Path, request: &Path, output: &Path) -> Result<Output, String> {
    run_args(
        bundle,
        &[
            "--request".to_owned(),
            request.to_str().ok_or("request path")?.to_owned(),
            "--output".to_owned(),
            output.to_str().ok_or("output path")?.to_owned(),
        ],
        None,
    )
}

fn spawn_bundle(bundle: &Path, request: &Path, output: &Path) -> Result<std::process::Child, String> {
    Command::new(bundle)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .arg("--request")
        .arg(request)
        .arg("--output")
        .arg(output)
        .spawn()
        .map_err(|err| format!("spawn bundle {}", kind_name(err.kind())))
}

fn run_args(bin: &Path, args: &[String], stdin_bytes: Option<&[u8]>) -> Result<Output, String> {
    let mut command = Command::new(bin);
    command.env_clear().args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    if stdin_bytes.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }
    let mut child = command
        .spawn()
        .map_err(|err| format!("spawn {}", kind_name(err.kind())))?;
    if let Some(bytes) = stdin_bytes {
        let mut stdin = child.stdin.take().ok_or("stdin")?;
        stdin.write_all(bytes).map_err(|_| "stdin write".to_owned())?;
    }
    child.wait_with_output().map_err(|_| "wait".to_owned())
}

fn expect_success(output: &Output) -> Result<(), String> {
    if output.status.code() != Some(0) || !output.stdout.is_empty() || !output.stderr.is_empty() {
        return Err(format!(
            "publish exit={} stderr={}",
            output.status.code().unwrap_or(-1),
            show_stderr(&output.stderr)
        ));
    }
    Ok(())
}

fn expect_exact(output: &Output, code: i32, line: &str) -> Result<(), String> {
    if output.status.code() != Some(code) || !output.stdout.is_empty() {
        return Err(format!(
            "exit={} want={code} stderr={}",
            output.status.code().unwrap_or(-1),
            show_stderr(&output.stderr)
        ));
    }
    let stderr = String::from_utf8(output.stderr.clone()).map_err(|_| "stderr utf8".to_owned())?;
    if stderr != format!("{line}\n") {
        return Err(format!("stderr {}", show_stderr(&output.stderr)));
    }
    Ok(())
}

fn probe_hard_link(dir: &Path) -> Result<String, String> {
    fresh_dir(dir)?;
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::write(&src, b"PROBE-a238").map_err(|_| "probe write".to_owned())?;
    match fs::hard_link(&src, &dst) {
        Ok(()) => {
            let left = fs::symlink_metadata(&src).map_err(|_| "probe src meta".to_owned())?;
            let right = fs::symlink_metadata(&dst).map_err(|_| "probe dst meta".to_owned())?;
            let eq = left.ino() == right.ino();
            let nlink = left.nlink();
            let _ = fs::remove_file(&dst);
            let _ = fs::remove_file(&src);
            let _ = fs::remove_dir(dir);
            Ok(format!("supported ino_eq={eq} nlink={nlink}"))
        }
        Err(err) => {
            let _ = fs::remove_file(&src);
            Ok(format!(
                "failed kind={} raw={}",
                kind_name(err.kind()),
                err.raw_os_error().map(|code| code.to_string()).unwrap_or_else(|| "none".to_owned())
            ))
        }
    }
}

fn fresh_dir(dir: &Path) -> Result<(), String> {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).map_err(|err| format!("mkdir {}", kind_name(err.kind())))
}

fn staging_of(dest: &Path) -> PathBuf {
    let name = dest.file_name().and_then(|item| item.to_str()).unwrap_or("out");
    dest.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{name}.role-pack-bundle-staging"))
}

fn show_stderr(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    if text.len() > 300
        || text.contains('/')
        || text.contains("prin-")
        || text.contains("tile-mark")
        || text.contains("hearth-mark")
        || text.contains("pad-mark")
    {
        return format!("hidden len={}", bytes.len());
    }
    text.replace('\n', "\\n")
}

fn value_of(lines: &[String], key: &str) -> String {
    lines
        .iter()
        .find_map(|line| line.strip_prefix(&format!("{key}=")).map(str::to_owned))
        .unwrap_or_default()
}

fn flush(path: &Path, lines: &[String]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| "report dir".to_owned())?;
    }
    let mut body = lines.join("\n");
    body.push('\n');
    fs::write(path, body).map_err(|_| "report write".to_owned())
}

fn kind_name(kind: std::io::ErrorKind) -> &'static str {
    use std::io::ErrorKind;
    match kind {
        ErrorKind::NotFound => "not_found",
        ErrorKind::PermissionDenied => "permission_denied",
        ErrorKind::AlreadyExists => "already_exists",
        ErrorKind::Unsupported => "unsupported",
        ErrorKind::InvalidInput => "invalid_input",
        ErrorKind::WouldBlock => "would_block",
        _ => "other",
    }
}

fn self_check() -> Result<(), String> {
    if hex(&sha256(b"")) != EMPTY_SHA || hex(&sha256(b"abc")) != ABC_SHA {
        return Err("sha256 self-check failed".to_owned());
    }
    let sample = r#"{"note":"a}b","pack":{"x":"y}z"},"pack_sha256":"ab"}"#;
    if field_raw(sample, "pack")? != r#"{"x":"y}z"}"# {
        return Err("scanner pack span".to_owned());
    }
    if string_field(sample, "pack_sha256")? != "ab" {
        return Err("scanner string".to_owned());
    }
    if string_field(sample, "note")? != "a}b" {
        return Err("scanner braces inside a string".to_owned());
    }
    let _ = Duration::from_millis(0);
    Ok(())
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn string_field(doc: &str, key: &str) -> Result<String, String> {
    let raw = field_raw(doc, key)?;
    parse_json_string(&raw)
}

fn parse_json_string(raw: &str) -> Result<String, String> {
    let bytes = raw.as_bytes();
    if bytes.first() != Some(&b'"') || bytes.last() != Some(&b'"') {
        return Err("not a json string".to_owned());
    }
    let mut out = String::new();
    let mut index = 1;
    while index + 1 < bytes.len() {
        if bytes[index] == b'\\' {
            index += 1;
            if index >= bytes.len() {
                return Err("bad escape".to_owned());
            }
            match bytes[index] {
                b'"' => out.push('"'),
                b'\\' => out.push('\\'),
                b'n' => out.push('\n'),
                b't' => out.push('\t'),
                other => out.push(other as char),
            }
            index += 1;
            continue;
        }
        let width = utf8_width(bytes[index]);
        if index + width >= bytes.len() {
            return Err("bad utf8".to_owned());
        }
        out.push_str(std::str::from_utf8(&bytes[index..index + width]).map_err(|_| "utf8")?);
        index += width;
    }
    Ok(out)
}

fn utf8_width(byte: u8) -> usize {
    if byte < 0x80 {
        1
    } else if byte < 0xe0 {
        2
    } else if byte < 0xf0 {
        3
    } else {
        4
    }
}

fn field_raw(doc: &str, want: &str) -> Result<String, String> {
    let bytes = doc.as_bytes();
    let mut index = 0;
    skip_ws(bytes, &mut index);
    if bytes.get(index) != Some(&b'{') {
        return Err(format!("object missing for {want}"));
    }
    index += 1;
    loop {
        skip_ws(bytes, &mut index);
        if bytes.get(index) == Some(&b'}') {
            return Err(format!("field absent {want}"));
        }
        let key = parse_key(bytes, &mut index)?;
        skip_ws(bytes, &mut index);
        if bytes.get(index) != Some(&b':') {
            return Err("colon".to_owned());
        }
        index += 1;
        skip_ws(bytes, &mut index);
        let start = index;
        skip_value(bytes, &mut index)?;
        if key == want {
            return Ok(doc[start..index].to_owned());
        }
        skip_ws(bytes, &mut index);
        match bytes.get(index) {
            Some(&b',') => index += 1,
            Some(&b'}') => {}
            _ => return Err("comma".to_owned()),
        }
    }
}

fn object_keys(doc: &str) -> Result<Vec<String>, String> {
    let bytes = doc.as_bytes();
    let mut index = 0;
    let mut keys = Vec::new();
    skip_ws(bytes, &mut index);
    if bytes.get(index) != Some(&b'{') {
        return Err("keys object".to_owned());
    }
    index += 1;
    loop {
        skip_ws(bytes, &mut index);
        if bytes.get(index) == Some(&b'}') {
            return Ok(keys);
        }
        keys.push(parse_key(bytes, &mut index)?);
        skip_ws(bytes, &mut index);
        if bytes.get(index) != Some(&b':') {
            return Err("keys colon".to_owned());
        }
        index += 1;
        skip_ws(bytes, &mut index);
        skip_value(bytes, &mut index)?;
        skip_ws(bytes, &mut index);
        match bytes.get(index) {
            Some(&b',') => index += 1,
            Some(&b'}') => {}
            _ => return Err("keys comma".to_owned()),
        }
    }
}

fn parse_key(bytes: &[u8], index: &mut usize) -> Result<String, String> {
    let start = *index;
    skip_string(bytes, index)?;
    parse_json_string(std::str::from_utf8(&bytes[start..*index]).map_err(|_| "key utf8")?)
}

fn skip_ws(bytes: &[u8], index: &mut usize) {
    while let Some(byte) = bytes.get(*index) {
        if !matches!(byte, b' ' | b'\n' | b'\r' | b'\t') {
            break;
        }
        *index += 1;
    }
}

fn skip_value(bytes: &[u8], index: &mut usize) -> Result<(), String> {
    match bytes.get(*index).copied() {
        Some(b'"') => skip_string(bytes, index),
        Some(b'{') => skip_container(bytes, index, b'{', b'}'),
        Some(b'[') => skip_container(bytes, index, b'[', b']'),
        Some(b't') => skip_literal(bytes, index, b"true"),
        Some(b'f') => skip_literal(bytes, index, b"false"),
        Some(b'n') => skip_literal(bytes, index, b"null"),
        Some(b'-') | Some(b'0'..=b'9') => skip_number(bytes, index),
        _ => Err("value".to_owned()),
    }
}

fn skip_container(bytes: &[u8], index: &mut usize, open: u8, close: u8) -> Result<(), String> {
    if bytes.get(*index) != Some(&open) {
        return Err("container".to_owned());
    }
    *index += 1;
    let mut depth = 1usize;
    while *index < bytes.len() && depth > 0 {
        match bytes[*index] {
            b'"' => skip_string(bytes, index)?,
            byte if byte == open => {
                depth += 1;
                *index += 1;
            }
            byte if byte == close => {
                depth -= 1;
                *index += 1;
            }
            _ => *index += 1,
        }
    }
    if depth != 0 {
        return Err("container end".to_owned());
    }
    Ok(())
}

fn skip_literal(bytes: &[u8], index: &mut usize, literal: &[u8]) -> Result<(), String> {
    if bytes.get(*index..*index + literal.len()) != Some(literal) {
        return Err("literal".to_owned());
    }
    *index += literal.len();
    Ok(())
}

fn skip_number(bytes: &[u8], index: &mut usize) -> Result<(), String> {
    let start = *index;
    if bytes.get(*index) == Some(&b'-') {
        *index += 1;
    }
    while matches!(bytes.get(*index), Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')) {
        *index += 1;
    }
    if *index == start {
        return Err("number".to_owned());
    }
    Ok(())
}

fn skip_string(bytes: &[u8], index: &mut usize) -> Result<(), String> {
    if bytes.get(*index) != Some(&b'"') {
        return Err("string".to_owned());
    }
    *index += 1;
    while *index < bytes.len() {
        match bytes[*index] {
            b'"' => {
                *index += 1;
                return Ok(());
            }
            b'\\' => {
                *index += 1;
                if *index >= bytes.len() {
                    return Err("escape".to_owned());
                }
                if bytes[*index] == b'u' {
                    *index += 5;
                    if *index > bytes.len() {
                        return Err("unicode escape".to_owned());
                    }
                } else {
                    *index += 1;
                }
            }
            _ => *index += 1,
        }
    }
    Err("unterminated string".to_owned())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
        0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
        0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
        0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
        0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
        0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
        0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut state = [
        0x6a09_e667_u32,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let bit_len = u64::try_from(input.len())
        .expect("len")
        .checked_mul(8)
        .expect("bits");
    let mut msg = input.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut word = [0u32; 64];
        for (index, slot) in word.iter_mut().enumerate().take(16) {
            let start = index * 4;
            *slot = u32::from_be_bytes(chunk[start..start + 4].try_into().unwrap());
        }
        for index in 16..64 {
            let earlier = word[index - 15];
            let later = word[index - 2];
            let s0 = earlier.rotate_right(7) ^ earlier.rotate_right(18) ^ (earlier >> 3);
            let s1 = later.rotate_right(17) ^ later.rotate_right(19) ^ (later >> 10);
            word[index] = word[index - 16]
                .wrapping_add(s0)
                .wrapping_add(word[index - 7])
                .wrapping_add(s1);
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(word[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut out = [0u8; 32];
    for (index, value) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}
