//! Local bundle for one role-pack command result.
//!
//! The request is caller-supplied public data. This binary calls
//! `compile_role_pack_command` and stores that result's artifact and manifest
//! bytes. It does not authenticate, grant, mount, tokenize, trust a cache, or
//! publish. The output parent is a trusted path, not a jail.

#![forbid(unsafe_code)]

use std::env;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use oo_builder::{RolePackCommandError, compile_role_pack_command};
use oo_core::limits::DOCUMENT_MAX_BYTES;
use serde_json::Value;

const PROTOCOL: &str = "oo.builder.role-pack-bundle/v1";
const SOURCE_PROTOCOL: &str = "oo.builder.role-pack-command/v1";
const PACK_PROTOCOL: &str = "oo.builder.role-pack/v1";
const MANIFEST_PROTOCOL: &str = "oo.builder.role-pack-manifest/v1";
const COMPILER: &str = "oo-builder/role-pack/v1";
const STAGING_SUFFIX: &str = ".role-pack-bundle-staging";
const NAME_MAX: usize = 255;
const BUNDLE_MAX_BYTES: usize = DOCUMENT_MAX_BYTES * 8;
// Linux link(2) uses EPERM when the filesystem does not support hard links.
const LINUX_LINK_EPERM: i32 = 1;

const HELP: &str = "\
role-pack-bundle reads one oo.builder.role-pack-command/v1 file and writes one new bundle file.
usage: role-pack-bundle --request <file> --output <file>
       role-pack-bundle --help
The request is caller-supplied public data. This command calls compile_role_pack_command.
It does not authenticate, grant, mount, tokenize, trust a cache, or publish.
The output parent is a trusted path, not a jail. Symlinks are not followed.
The parent directory is not fsynced. A crash can leave a staging file or an undurable directory entry.
Staging uses create_new and sync_all. Publication is a same-directory hard link.
An existing output path is not replaced. An existing staging path is refused.
A filesystem that does not support hard links fails publication.
On failure only the staging file created by this attempt is removed.";

#[derive(Debug)]
struct Fail {
    code: String,
    message: &'static str,
    exit: u8,
}

impl Fail {
    fn bundle(name: &'static str, message: &'static str) -> Self {
        Self {
            code: format!("{PROTOCOL}/{name}"),
            message,
            exit: 1,
        }
    }

    fn usage() -> Self {
        Self {
            code: format!("{PROTOCOL}/usage"),
            message: "arguments are --request, --output, or --help",
            exit: 2,
        }
    }
}

fn identity() -> Fail {
    Fail::bundle("bundle.identity", "compiled output is missing an identity")
}

fn status() -> Fail {
    Fail::bundle(
        "bundle.status",
        "compiled manifest status was not left unchanged",
    )
}

enum Args {
    Help,
    Run { request: String, output: String },
}

fn main() -> ExitCode {
    match run() {
        Ok(Args::Help) => {
            println!("{HELP}");
            ExitCode::SUCCESS
        }
        Ok(Args::Run { .. }) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("role-pack-bundle: {} {}", err.code, err.message);
            ExitCode::from(err.exit)
        }
    }
}

fn run() -> Result<Args, Fail> {
    let args = parse_args()?;
    if let Args::Run { request, output } = &args {
        let text = read_request(Path::new(request))?;
        let command = compile_role_pack_command(&text).map_err(from_command)?;
        let bundle = bundle_document(&command)?;
        publish_new(Path::new(output), bundle.as_bytes())?;
    }
    Ok(args)
}

fn from_command(err: RolePackCommandError) -> Fail {
    Fail {
        code: err.code,
        message: err.message,
        exit: 1,
    }
}

fn parse_args() -> Result<Args, Fail> {
    let mut request = None;
    let mut output = None;
    let mut help = false;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => help = true,
            "--request" => {
                if request.is_some() {
                    return Err(Fail::usage());
                }
                request = Some(args.next().ok_or_else(Fail::usage)?);
            }
            "--output" => {
                if output.is_some() {
                    return Err(Fail::usage());
                }
                output = Some(args.next().ok_or_else(Fail::usage)?);
            }
            _ => return Err(Fail::usage()),
        }
    }
    if help {
        if request.is_some() || output.is_some() {
            return Err(Fail::usage());
        }
        return Ok(Args::Help);
    }
    match (request, output) {
        (Some(request), Some(output)) => Ok(Args::Run { request, output }),
        _ => Err(Fail::usage()),
    }
}

fn read_request(path: &Path) -> Result<String, Fail> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return Err(Fail::bundle("io.missing", "the request file is missing"));
        }
        Err(_) => return Err(Fail::bundle("io.read", "the request file could not be read")),
    };
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return Err(Fail::bundle(
            "io.kind",
            "the request path is not a regular file",
        ));
    }
    let mut file = File::open(path)
        .map_err(|_| Fail::bundle("io.read", "the request file could not be read"))?;
    let mut buf = Vec::new();
    let max = u64::try_from(DOCUMENT_MAX_BYTES.saturating_add(1)).unwrap_or(u64::MAX);
    Read::take(&mut file, max)
        .read_to_end(&mut buf)
        .map_err(|_| Fail::bundle("io.read", "the request file could not be read"))?;
    if buf.len() > DOCUMENT_MAX_BYTES {
        return Err(Fail::bundle(
            "json.budget.bytes",
            "document exceeds the byte budget",
        ));
    }
    let text = std::str::from_utf8(&buf)
        .map_err(|_| Fail::bundle("json.syntax", "the request file is not UTF-8"))?;
    Ok(text.to_string())
}

fn bundle_document(command: &str) -> Result<String, Fail> {
    let pack_raw = field_span(command, "artifact")?;
    let manifest_raw = field_span(command, "manifest")?;
    if !pack_raw.starts_with('{') || !manifest_raw.starts_with('{') {
        return Err(identity());
    }
    let value: Value = serde_json::from_str(command).map_err(|_| identity())?;
    let pack_value: Value = serde_json::from_str(pack_raw).map_err(|_| identity())?;
    let manifest_value: Value = serde_json::from_str(manifest_raw).map_err(|_| identity())?;
    if value.get("artifact") != Some(&pack_value) || value.get("manifest") != Some(&manifest_value)
    {
        return Err(identity());
    }
    check_protocols(&value, &pack_value, &manifest_value)?;
    let compiler = shared_compiler(&pack_value, &manifest_value)?;
    let pack_sha = sha256_hex(pack_raw.as_bytes());
    let manifest_sha = sha256_hex(manifest_raw.as_bytes());
    let pack_digest = copied_digest(&value, &pack_value)?;
    check_manifest_hash(&value, &pack_value, &manifest_sha)?;
    check_statuses(&manifest_value)?;
    let snapshot = snapshot_object(&manifest_value)?;
    let statuses = statuses_object(&manifest_value)?;
    let pack_length = u64::try_from(pack_raw.len()).map_err(|_| identity())?;
    let manifest_length = u64::try_from(manifest_raw.len()).map_err(|_| identity())?;
    let out = assemble(&BundleParts {
        compiler,
        manifest_raw,
        manifest_length,
        manifest_sha: &manifest_sha,
        pack_raw,
        pack_digest,
        pack_length,
        pack_sha: &pack_sha,
        snapshot: &snapshot,
        statuses: &statuses,
    });
    if out.len() > BUNDLE_MAX_BYTES {
        return Err(Fail::bundle(
            "bundle.budget",
            "bundle exceeds the byte budget",
        ));
    }
    Ok(out)
}

fn check_protocols(value: &Value, pack_value: &Value, manifest_value: &Value) -> Result<(), Fail> {
    require_text(value, "protocol", SOURCE_PROTOCOL)?;
    require_text(pack_value, "protocol", PACK_PROTOCOL)?;
    require_text(manifest_value, "protocol", MANIFEST_PROTOCOL)?;
    Ok(())
}

fn shared_compiler<'a>(pack_value: &'a Value, manifest_value: &'a Value) -> Result<&'a str, Fail> {
    let compiler = text_field(pack_value, "compiler")?;
    if compiler != text_field(manifest_value, "compiler")? || compiler != COMPILER {
        return Err(identity());
    }
    Ok(compiler)
}

fn copied_digest<'a>(value: &'a Value, pack_value: &Value) -> Result<&'a str, Fail> {
    let digest = text_field(value, "pack_digest")?;
    if digest != text_field(pack_value, "pack_digest")? {
        return Err(identity());
    }
    Ok(digest)
}

fn check_manifest_hash(value: &Value, pack_value: &Value, manifest_sha: &str) -> Result<(), Fail> {
    let field = text_field(value, "manifest_sha256")?;
    if field != manifest_sha || field != text_field(pack_value, "manifest_sha256")? {
        return Err(identity());
    }
    Ok(())
}

fn require_text(value: &Value, key: &str, expected: &str) -> Result<(), Fail> {
    if text_field(value, key)? == expected {
        Ok(())
    } else {
        Err(identity())
    }
}

fn text_field<'a>(value: &'a Value, key: &str) -> Result<&'a str, Fail> {
    value.get(key).and_then(Value::as_str).ok_or_else(identity)
}

fn check_statuses(manifest: &Value) -> Result<(), Fail> {
    require_status(manifest, "authentication", "not_claimed")?;
    require_status(manifest, "authorization", "not_claimed")?;
    require_status(manifest, "publication", "not_claimed")?;
    require_status(manifest, "mount", "not_performed")?;
    require_status(manifest, "samples", "omitted")?;
    require_status(manifest, "domains", "unsupported")?;
    require_status(manifest, "file_splitting", "unsupported")?;
    require_status(manifest, "search", "unsupported")?;
    if manifest.get("row_security_reproven") != Some(&Value::Bool(false)) {
        return Err(status());
    }
    let tokens = manifest.get("tokens").ok_or_else(status)?;
    require_status(tokens, "status", "unsupported")?;
    require_status(tokens, "estimator", "none")?;
    Ok(())
}

fn require_status(value: &Value, key: &str, expected: &str) -> Result<(), Fail> {
    if value.get(key).and_then(Value::as_str) == Some(expected) {
        Ok(())
    } else {
        Err(status())
    }
}

fn snapshot_object(manifest: &Value) -> Result<String, Fail> {
    let policy = manifest.get("policy").ok_or_else(identity)?;
    let knowledge = manifest.get("knowledge").ok_or_else(identity)?;
    let mut out = String::from("{");
    let mut first = true;
    push_str(&mut out, &mut first, "catalog_version", text_field(manifest, "catalog_version")?);
    push_str(&mut out, &mut first, "knowledge_id", text_field(knowledge, "id")?);
    push_str(
        &mut out,
        &mut first,
        "knowledge_version",
        text_field(knowledge, "version")?,
    );
    push_str(
        &mut out,
        &mut first,
        "ontology_version",
        text_field(manifest, "ontology_version")?,
    );
    push_str(&mut out, &mut first, "policy_id", text_field(policy, "id")?);
    push_str(&mut out, &mut first, "policy_version", text_field(policy, "version")?);
    push_str(&mut out, &mut first, "project", text_field(manifest, "project")?);
    push_str(&mut out, &mut first, "role", text_field(manifest, "role")?);
    out.push('}');
    Ok(out)
}

fn statuses_object(manifest: &Value) -> Result<String, Fail> {
    let tokens = manifest.get("tokens").ok_or_else(status)?;
    let mut out = String::from("{");
    let mut first = true;
    push_str(&mut out, &mut first, "authentication", "not_claimed");
    push_str(&mut out, &mut first, "authorization", "not_claimed");
    push_str(&mut out, &mut first, "domains", "unsupported");
    push_str(&mut out, &mut first, "file_splitting", "unsupported");
    push_str(&mut out, &mut first, "mount", "not_performed");
    push_str(&mut out, &mut first, "publication", "not_claimed");
    push_bool(&mut out, &mut first, "row_security_reproven", false);
    push_str(&mut out, &mut first, "samples", "omitted");
    push_str(&mut out, &mut first, "search", "unsupported");
    push_str(
        &mut out,
        &mut first,
        "tokens_estimator",
        text_field(tokens, "estimator")?,
    );
    push_str(&mut out, &mut first, "tokens_status", text_field(tokens, "status")?);
    out.push('}');
    Ok(out)
}

struct BundleParts<'a> {
    compiler: &'a str,
    manifest_raw: &'a str,
    manifest_length: u64,
    manifest_sha: &'a str,
    pack_raw: &'a str,
    pack_digest: &'a str,
    pack_length: u64,
    pack_sha: &'a str,
    snapshot: &'a str,
    statuses: &'a str,
}

fn assemble(parts: &BundleParts<'_>) -> String {
    let mut out = String::new();
    let mut first = true;
    out.push('{');
    push_str(&mut out, &mut first, "compiler", parts.compiler);
    push_raw(&mut out, &mut first, "manifest", parts.manifest_raw);
    push_u64(&mut out, &mut first, "manifest_length", parts.manifest_length);
    push_str(&mut out, &mut first, "manifest_sha256", parts.manifest_sha);
    push_raw(&mut out, &mut first, "pack", parts.pack_raw);
    push_str(&mut out, &mut first, "pack_digest", parts.pack_digest);
    push_u64(&mut out, &mut first, "pack_length", parts.pack_length);
    push_str(&mut out, &mut first, "pack_protocol", PACK_PROTOCOL);
    push_str(&mut out, &mut first, "pack_sha256", parts.pack_sha);
    push_str(&mut out, &mut first, "protocol", PROTOCOL);
    push_raw(&mut out, &mut first, "snapshot", parts.snapshot);
    push_str(&mut out, &mut first, "source_protocol", SOURCE_PROTOCOL);
    push_raw(&mut out, &mut first, "statuses", parts.statuses);
    out.push('}');
    out
}

fn push_sep(out: &mut String, first: &mut bool) {
    if !*first {
        out.push(',');
    }
    *first = false;
}

fn push_key(out: &mut String, first: &mut bool, key: &str) {
    push_sep(out, first);
    out.push('"');
    out.push_str(key);
    out.push_str("\":");
}

fn push_str(out: &mut String, first: &mut bool, key: &str, value: &str) {
    push_key(out, first, key);
    push_json_string(out, value);
}

fn push_raw(out: &mut String, first: &mut bool, key: &str, raw: &str) {
    push_key(out, first, key);
    out.push_str(raw);
}

fn push_u64(out: &mut String, first: &mut bool, key: &str, value: u64) {
    push_key(out, first, key);
    let _ = write!(out, "{value}");
}

fn push_bool(out: &mut String, first: &mut bool, key: &str, value: bool) {
    push_key(out, first, key);
    out.push_str(if value { "true" } else { "false" });
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(ch));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = oo_core::canonical::hash_bytes(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn publish_new(dest: &Path, bytes: &[u8]) -> Result<(), Fail> {
    let staging = staging_path(dest)?;
    ensure_directory(&parent_dir(dest)?)?;
    if path_exists(
        &staging,
        "io.collision",
        "the staging path could not be inspected",
    )? {
        return Err(Fail::bundle(
            "io.collision",
            "the staging path already exists",
        ));
    }
    create_and_write(&staging, bytes)?;
    finish_owned(&staging, dest, bytes)
}

fn parent_dir(dest: &Path) -> Result<PathBuf, Fail> {
    let text = dest.to_str().ok_or_else(name_err)?;
    if text.is_empty() || text.ends_with('/') || text.ends_with('\\') {
        return Err(name_err());
    }
    match dest.parent() {
        Some(parent) if parent.as_os_str().is_empty() => Ok(PathBuf::from(".")),
        Some(parent) => Ok(parent.to_path_buf()),
        None => Err(name_err()),
    }
}

fn name_err() -> Fail {
    Fail::bundle(
        "io.name",
        "the output file name does not fit the staging suffix",
    )
}

fn staging_path(dest: &Path) -> Result<PathBuf, Fail> {
    let name = file_name(dest)?;
    Ok(parent_dir(dest)?.join(format!("{name}{STAGING_SUFFIX}")))
}

fn file_name(dest: &Path) -> Result<&str, Fail> {
    let name = dest
        .file_name()
        .and_then(|item| item.to_str())
        .ok_or_else(name_err)?;
    if name.is_empty() || name == "." || name == ".." {
        return Err(name_err());
    }
    if name.len().saturating_add(STAGING_SUFFIX.len()) > NAME_MAX {
        return Err(name_err());
    }
    Ok(name)
}

fn ensure_directory(path: &Path) -> Result<(), Fail> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_dir() && !meta.file_type().is_symlink() => Ok(()),
        _ => Err(Fail::bundle(
            "io.parent",
            "the output parent is not a directory",
        )),
    }
}

fn path_exists(path: &Path, code: &'static str, message: &'static str) -> Result<bool, Fail> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(_) => Err(Fail::bundle(code, message)),
    }
}

fn create_and_write(staging: &Path, bytes: &[u8]) -> Result<(), Fail> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(staging) {
        Ok(file) => file,
        Err(err) if err.kind() == ErrorKind::AlreadyExists => {
            return Err(Fail::bundle(
                "io.collision",
                "the staging path already exists",
            ));
        }
        Err(_) => {
            return Err(Fail::bundle(
                "io.staging",
                "the staging file could not be created",
            ));
        }
    };
    let wrote = write_synced(&mut file, bytes);
    drop(file);
    if let Err(err) = wrote {
        return Err(prefer_cleanup(staging, err));
    }
    Ok(())
}

fn write_synced(file: &mut File, bytes: &[u8]) -> Result<(), Fail> {
    file.write_all(bytes)
        .map_err(|_| Fail::bundle("io.write", "the staging file could not be written"))?;
    file.flush()
        .map_err(|_| Fail::bundle("io.write", "the staging file could not be written"))?;
    file.sync_all()
        .map_err(|_| Fail::bundle("io.sync", "the staging file could not be synced"))?;
    Ok(())
}

fn finish_owned(staging: &Path, dest: &Path, bytes: &[u8]) -> Result<(), Fail> {
    if let Err(err) = readback_and_link(staging, dest, bytes) {
        return Err(prefer_cleanup(staging, err));
    }
    cleanup_owned(staging)
}

fn readback_and_link(staging: &Path, dest: &Path, bytes: &[u8]) -> Result<(), Fail> {
    readback(staging, bytes)?;
    link_no_replace(staging, dest)
}

fn link_no_replace(staging: &Path, dest: &Path) -> Result<(), Fail> {
    match std::fs::hard_link(staging, dest) {
        Ok(()) => Ok(()),
        Err(err) => Err(link_failure(err)),
    }
}

fn link_failure(err: std::io::Error) -> Fail {
    if err.kind() == ErrorKind::AlreadyExists {
        return Fail::bundle("io.destination", "the output path already exists");
    }
    if hard_link_unsupported(&err) {
        return Fail::bundle("io.link", "the filesystem does not support a hard link");
    }
    Fail::bundle("io.link", "the staging file could not be linked")
}

fn hard_link_unsupported(err: &std::io::Error) -> bool {
    err.kind() == ErrorKind::Unsupported || err.raw_os_error() == Some(LINUX_LINK_EPERM)
}

fn readback(path: &Path, expected: &[u8]) -> Result<(), Fail> {
    let mut file = File::open(path)
        .map_err(|_| Fail::bundle("io.readback", "the staging file could not be read"))?;
    let mut buf = Vec::new();
    let max = u64::try_from(expected.len().saturating_add(1)).unwrap_or(u64::MAX);
    Read::take(&mut file, max)
        .read_to_end(&mut buf)
        .map_err(|_| Fail::bundle("io.readback", "the staging file could not be read"))?;
    if buf.as_slice() != expected {
        return Err(Fail::bundle(
            "io.readback",
            "the staging file did not match the bundle",
        ));
    }
    Ok(())
}

fn prefer_cleanup(staging: &Path, err: Fail) -> Fail {
    match cleanup_owned(staging) {
        Ok(()) => err,
        Err(cleanup) => cleanup,
    }
}

fn cleanup_owned(path: &Path) -> Result<(), Fail> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Fail::bundle(
            "io.cleanup",
            "the staging file could not be removed",
        )),
    }
}

fn field_span<'a>(document: &'a str, want: &str) -> Result<&'a str, Fail> {
    let bytes = document.as_bytes();
    let mut index = 0;
    skip_ws(bytes, &mut index);
    if bytes.get(index) != Some(&b'{') {
        return Err(identity());
    }
    index += 1;
    loop {
        skip_ws(bytes, &mut index);
        if bytes.get(index) == Some(&b'}') {
            return Err(identity());
        }
        let key = parse_key(bytes, &mut index)?;
        skip_ws(bytes, &mut index);
        if bytes.get(index) != Some(&b':') {
            return Err(identity());
        }
        index += 1;
        skip_ws(bytes, &mut index);
        let start = index;
        skip_value(bytes, &mut index)?;
        if !document.is_char_boundary(start) || !document.is_char_boundary(index) {
            return Err(identity());
        }
        if key == want {
            return Ok(&document[start..index]);
        }
        skip_ws(bytes, &mut index);
        match bytes.get(index) {
            Some(b',') => index += 1,
            _ => return Err(identity()),
        }
    }
}

fn parse_key(bytes: &[u8], index: &mut usize) -> Result<String, Fail> {
    let start = *index;
    skip_string(bytes, index)?;
    let raw = std::str::from_utf8(&bytes[start..*index]).map_err(|_| identity())?;
    decode_json_string(raw)
}

fn decode_json_string(quoted: &str) -> Result<String, Fail> {
    let bytes = quoted.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || bytes[bytes.len() - 1] != b'"' {
        return Err(identity());
    }
    let mut out = String::new();
    let mut index = 1;
    while index < bytes.len() - 1 {
        if bytes[index] == b'\\' {
            index += 1;
            if index >= bytes.len() - 1 {
                return Err(identity());
            }
            match bytes[index] {
                b'"' => out.push('"'),
                b'\\' => out.push('\\'),
                b'/' => out.push('/'),
                b'b' => out.push('\u{0008}'),
                b'f' => out.push('\u{000c}'),
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                b'u' => out.push(decode_hex(bytes, &mut index)?),
                _ => return Err(identity()),
            }
            index += 1;
        } else {
            let rest = std::str::from_utf8(&bytes[index..bytes.len() - 1]).map_err(|_| identity())?;
            let ch = rest.chars().next().ok_or_else(identity)?;
            out.push(ch);
            index += ch.len_utf8();
        }
    }
    Ok(out)
}

fn decode_hex(bytes: &[u8], index: &mut usize) -> Result<char, Fail> {
    if *index + 4 >= bytes.len() - 1 {
        return Err(identity());
    }
    let hex = std::str::from_utf8(&bytes[*index + 1..*index + 5]).map_err(|_| identity())?;
    let code = u32::from_str_radix(hex, 16).map_err(|_| identity())?;
    *index += 4;
    char::from_u32(code).ok_or_else(identity)
}

fn skip_ws(bytes: &[u8], index: &mut usize) {
    while matches!(bytes.get(*index), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        *index += 1;
    }
}

fn skip_value(bytes: &[u8], index: &mut usize) -> Result<(), Fail> {
    skip_ws(bytes, index);
    match bytes.get(*index).copied() {
        Some(b'{') => skip_container(bytes, index, b'{', b'}'),
        Some(b'[') => skip_container(bytes, index, b'[', b']'),
        Some(b'"') => skip_string(bytes, index),
        Some(b't') => skip_word(bytes, index, b"true"),
        Some(b'f') => skip_word(bytes, index, b"false"),
        Some(b'n') => skip_word(bytes, index, b"null"),
        Some(b'-' | b'0'..=b'9') => skip_number(bytes, index),
        _ => Err(identity()),
    }
}

fn skip_container(bytes: &[u8], index: &mut usize, open: u8, close: u8) -> Result<(), Fail> {
    if bytes.get(*index) != Some(&open) {
        return Err(identity());
    }
    *index += 1;
    let mut depth = 1_usize;
    while *index < bytes.len() {
        match bytes[*index] {
            b'"' => skip_string(bytes, index)?,
            byte if byte == open => {
                depth += 1;
                *index += 1;
            }
            byte if byte == close => {
                depth -= 1;
                *index += 1;
                if depth == 0 {
                    return Ok(());
                }
            }
            _ => *index += 1,
        }
    }
    Err(identity())
}

fn skip_string(bytes: &[u8], index: &mut usize) -> Result<(), Fail> {
    if bytes.get(*index) != Some(&b'"') {
        return Err(identity());
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
                    return Err(identity());
                }
                if bytes[*index] == b'u' {
                    if *index + 4 >= bytes.len() {
                        return Err(identity());
                    }
                    *index += 5;
                } else {
                    *index += 1;
                }
            }
            _ => *index += 1,
        }
    }
    Err(identity())
}

fn skip_word(bytes: &[u8], index: &mut usize, word: &[u8]) -> Result<(), Fail> {
    if bytes.get(*index..).is_some_and(|rest| rest.starts_with(word)) {
        *index += word.len();
        Ok(())
    } else {
        Err(identity())
    }
}

fn skip_number(bytes: &[u8], index: &mut usize) -> Result<(), Fail> {
    if bytes.get(*index) == Some(&b'-') {
        *index += 1;
    }
    let digits = *index;
    while matches!(bytes.get(*index), Some(b'0'..=b'9')) {
        *index += 1;
    }
    if *index == digits {
        return Err(identity());
    }
    if bytes.get(*index) == Some(&b'.') {
        *index += 1;
        let fraction = *index;
        while matches!(bytes.get(*index), Some(b'0'..=b'9')) {
            *index += 1;
        }
        if *index == fraction {
            return Err(identity());
        }
    }
    if matches!(bytes.get(*index), Some(b'e' | b'E')) {
        *index += 1;
        if matches!(bytes.get(*index), Some(b'+' | b'-')) {
            *index += 1;
        }
        let exponent = *index;
        while matches!(bytes.get(*index), Some(b'0'..=b'9')) {
            *index += 1;
        }
        if *index == exponent {
            return Err(identity());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("oo-a214-{label}-{}-{n}", std::process::id()));
        fs::create_dir(&dir).expect("temp");
        dir
    }

    #[test]
    fn hash_bytes_matches_sha256_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn field_span_keeps_braces_inside_strings() {
        let doc = r#"{"artifact":{"n":1,"t":"a}b"},"manifest":{"ok":true}}"#;
        assert_eq!(field_span(doc, "artifact").expect("artifact"), r#"{"n":1,"t":"a}b"}"#);
        assert_eq!(field_span(doc, "manifest").expect("manifest"), r#"{"ok":true}"#);
    }

    #[test]
    fn publish_removes_staging_when_destination_exists() {
        let dir = temp_dir("dest");
        let dest = dir.join("out.json");
        let foreign = dir.join("foreign.txt");
        fs::write(&dest, b"KEEP-DEST").unwrap();
        fs::write(&foreign, b"KEEP-FOREIGN").unwrap();
        let err = publish_new(&dest, br#"{"protocol":"x"}"#).expect_err("exists");
        assert!(err.code.ends_with("/io.destination"), "{}", err.code);
        assert_eq!(fs::read(&dest).unwrap(), b"KEEP-DEST");
        assert_eq!(fs::read(&foreign).unwrap(), b"KEEP-FOREIGN");
        assert!(!dir.join("out.json.role-pack-bundle-staging").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn publish_leaves_a_preexisting_staging_path() {
        let dir = temp_dir("hit");
        let dest = dir.join("out.json");
        let staging = dir.join("out.json.role-pack-bundle-staging");
        fs::write(&staging, b"KEEP-STAGING").unwrap();
        let err = publish_new(&dest, b"{}").expect_err("collision");
        assert!(err.code.ends_with("/io.collision"), "{}", err.code);
        assert_eq!(fs::read(&staging).unwrap(), b"KEEP-STAGING");
        assert!(!dest.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn readback_mismatch_removes_only_the_staging_file() {
        let dir = temp_dir("back");
        let dest = dir.join("out.json");
        let staging = dir.join("owned.staging");
        let foreign = dir.join("foreign.txt");
        fs::write(&staging, b"short").unwrap();
        fs::write(&foreign, b"KEEP").unwrap();
        let err = finish_owned(&staging, &dest, b"expected-bytes").expect_err("readback");
        assert!(err.code.ends_with("/io.readback"), "{}", err.code);
        assert_eq!(err.message, "the staging file did not match the bundle");
        assert!(!staging.exists());
        assert!(!dest.exists());
        assert_eq!(fs::read(&foreign).unwrap(), b"KEEP");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn link_failure_removes_only_the_staging_file() {
        let dir = temp_dir("link");
        let staging = dir.join("owned.staging");
        let foreign = dir.join("foreign.txt");
        fs::write(&staging, b"bundle").unwrap();
        fs::write(&foreign, b"KEEP").unwrap();
        let dest = dir.join("missing-dir").join("out.json");
        let err = readback_and_link(&staging, &dest, b"bundle").expect_err("link");
        let err = prefer_cleanup(&staging, err);
        assert!(err.code.ends_with("/io.link"), "{}", err.code);
        assert_eq!(err.message, "the staging file could not be linked");
        assert!(!staging.exists());
        assert!(!dir.join("missing-dir").exists());
        assert_eq!(fs::read(&foreign).unwrap(), b"KEEP");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hard_link_shares_the_inode_and_then_drops_the_staging_name() {
        use std::os::unix::fs::MetadataExt;

        let dir = temp_dir("inode");
        let staging = dir.join("owned.staging");
        let dest = dir.join("out.json");
        fs::write(&staging, b"BUNDLE").unwrap();
        link_no_replace(&staging, &dest).expect("link");
        let linked = fs::metadata(&dest).unwrap();
        assert_eq!(linked.ino(), fs::metadata(&staging).unwrap().ino());
        assert!(linked.nlink() >= 2);
        cleanup_owned(&staging).expect("unlink staging");
        assert!(!staging.exists());
        assert_eq!(fs::read(&dest).unwrap(), b"BUNDLE");
        assert_eq!(fs::metadata(&dest).unwrap().ino(), linked.ino());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn existing_name_is_not_replaced() {
        let dir = temp_dir("gap");
        let staging = dir.join("owned.staging");
        let dest = dir.join("out.json");
        let foreign = dir.join("foreign.txt");
        fs::write(&staging, b"BUNDLE").unwrap();
        fs::write(&dest, b"SENTINEL-a221-gap").unwrap();
        fs::write(&foreign, b"KEEP").unwrap();
        let err = link_no_replace(&staging, &dest).expect_err("exists");
        assert!(err.code.ends_with("/io.destination"), "{}", err.code);
        assert_eq!(err.message, "the output path already exists");
        assert_eq!(fs::read(&dest).unwrap(), b"SENTINEL-a221-gap");
        assert_eq!(fs::read(&staging).unwrap(), b"BUNDLE");
        assert_eq!(fs::read(&foreign).unwrap(), b"KEEP");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn symlink_destination_is_not_replaced() {
        let dir = temp_dir("symlink");
        let staging = dir.join("owned.staging");
        let real = dir.join("real.json");
        let dest = dir.join("out.json");
        fs::write(&staging, b"BUNDLE").unwrap();
        fs::write(&real, b"KEEP-REAL").unwrap();
        std::os::unix::fs::symlink(&real, &dest).unwrap();
        let err = link_no_replace(&staging, &dest).expect_err("symlink");
        assert!(err.code.ends_with("/io.destination"), "{}", err.code);
        assert!(fs::symlink_metadata(&dest).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(&real).unwrap(), b"KEEP-REAL");
        assert_eq!(fs::read(&staging).unwrap(), b"BUNDLE");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn link_errors_use_static_sentences() {
        const LINUX_ENOENT: i32 = 2;
        const LINUX_EACCES: i32 = 13;
        const LINUX_EEXIST: i32 = 17;
        const LINUX_EXDEV: i32 = 18;
        const LINUX_EOPNOTSUPP: i32 = 95;

        let unsupported = link_failure(std::io::Error::from_raw_os_error(LINUX_LINK_EPERM));
        assert!(unsupported.code.ends_with("/io.link"), "{}", unsupported.code);
        assert_eq!(
            unsupported.message,
            "the filesystem does not support a hard link"
        );
        let opnotsupp = link_failure(std::io::Error::from_raw_os_error(LINUX_EOPNOTSUPP));
        assert_eq!(
            opnotsupp.message,
            "the filesystem does not support a hard link"
        );
        let exists = link_failure(std::io::Error::from_raw_os_error(LINUX_EEXIST));
        assert!(exists.code.ends_with("/io.destination"), "{}", exists.code);
        assert_eq!(exists.message, "the output path already exists");
        let denied = link_failure(std::io::Error::from_raw_os_error(LINUX_EACCES));
        assert_eq!(denied.message, "the staging file could not be linked");
        let missing = link_failure(std::io::Error::from_raw_os_error(LINUX_ENOENT));
        assert_eq!(missing.message, "the staging file could not be linked");
        let cross = link_failure(std::io::Error::from_raw_os_error(LINUX_EXDEV));
        assert_eq!(cross.message, "the staging file could not be linked");
    }

    #[test]
    fn cleanup_refuses_a_directory() {
        let dir = temp_dir("keepdir");
        let staging = dir.join("staging-dir");
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join("child.txt"), b"KEEP").unwrap();
        let err = cleanup_owned(&staging).expect_err("dir");
        assert!(err.code.ends_with("/io.cleanup"), "{}", err.code);
        assert_eq!(fs::read(staging.join("child.txt")).unwrap(), b"KEEP");
        let _ = fs::remove_dir_all(&dir);
    }
}
