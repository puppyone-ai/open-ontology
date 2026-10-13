use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use oo_pi_runtime_kit::cmap;
use oo_pi_runtime_kit::events::{self, Mapped};
use oo_pi_runtime_kit::handoff;
use oo_pi_runtime_kit::json;
use oo_pi_runtime_kit::manifest;
use oo_pi_runtime_kit::scan;
use oo_pi_runtime_kit::sha256;
use oo_pi_runtime_kit::{pin_value, PACK_MARKER, PI_VERSION, PUBLIC_MARKER};

#[test]
fn sha256_vectors() {
    assert_eq!(sha256::hex(&sha256::hash(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(sha256::hex(&sha256::hash(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

#[test]
fn pins_are_the_reviewed_values() {
    assert_eq!(pin_value("PI_VERSION").unwrap(), PI_VERSION);
    assert_eq!(pin_value("PI_COMMIT").unwrap(), "7c10bd4337495ee613f2224843ecdf349b80d1df");
    assert_eq!(pin_value("NODE_VERSION").unwrap(), "22.23.3");
    assert_eq!(pin_value("NODE_TARBALL_SHA").unwrap(), "5ced2d48d1d7198739b7f86804de0171aefb6823b684b12341d3321afc3cb0b2");
    assert_eq!(pin_value("NPM_TARBALL_SHA").unwrap(), "04910bdae661a6529e9d6869b04006f6aad01186398b04a16c6d9793667b96c5");
    assert_eq!(pin_value("RG_TARBALL_SHA").unwrap(), "c827481c4ff4ea10c9dc7a4022c8de5db34a5737cb74484d62eb94a95841ab2f");
    assert_eq!(pin_value("FONT_OTF_SHA").unwrap(), "2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b");
    assert_eq!(pin_value("PUBLIC_MARKER").unwrap(), PUBLIC_MARKER);
    assert_eq!(pin_value("PACK_MARKER").unwrap(), PACK_MARKER);
}

#[test]
fn json_round_trip_and_tool_events() {
    let started = r#"{"type":"message_update","assistantMessageEvent":{"type":"toolcall_end","toolCall":{"type":"toolCall","id":"call_fixed","name":"bash","arguments":{"command":"printf 'x'"}}}}"#;
    match events::map_line(started).unwrap() {
        Mapped::ToolStarted { name, call_id, args_summary } => {
            assert_eq!(name, "bash");
            assert_eq!(call_id, "call_fixed");
            assert_eq!(args_summary, r#"{"command":"printf 'x'"}"#);
        }
        other => panic!("mapped {other:?}"),
    }
    let finished = r#"{"type":"tool_execution_end","toolCallId":"call_fixed","toolName":"bash","result":{"content":[{"type":"text","text":"Only query is implemented"}]},"isError":false}"#;
    match events::map_line(finished).unwrap() {
        Mapped::ToolFinished { status, output_summary, .. } => {
            assert_eq!(status, "ok");
            assert!(output_summary.contains("Only query is implemented"));
        }
        other => panic!("mapped {other:?}"),
    }
    assert_eq!(events::map_line(r#"{"type":"tool_execution_start","toolCallId":"call_fixed"}"#).unwrap(), Mapped::Bookkeeping);
    assert!(matches!(events::map_line(r#"{"type":"extension_ui_request"}"#).unwrap(), Mapped::Unknown { .. }));
    assert!(events::response_ok(r#"{"id":"init-1","type":"response","success":true}"#, "init-1").unwrap());
    assert!(events::prompt_started(r#"{"id":"prompt-1","type":"response","command":"prompt","success":true,"data":{"disposition":"started"}}"#).unwrap());
    let parsed = json::parse(r#"{"ok":true,"n":1}"#).unwrap();
    assert_eq!(parsed.compact(), r#"{"ok":true,"n":1}"#);
}

#[test]
fn cmap_reports_the_unified_span() {
    let font = cmap::synthetic_format4(0x4E00, 0x4E02);
    let spans = cmap::cjk_spans(&font).unwrap();
    assert!(spans.iter().any(|span| span.block == "CJK Unified Ideographs" && span.start == 0x4E00 && span.end == 0x4E02));
    let text = cmap::spans_json(&spans);
    assert!(text.contains("4E00"));
    assert!(json::parse(&text).unwrap().get("ranges").is_some());
}

#[test]
fn secret_scan_allows_the_public_marker_only() {
    assert!(scan::secret_hit(b"apiKey public-proxy-marker").is_none());
    assert_eq!(scan::secret_hit(b"-----BEGIN PRIVATE KEY-----"), Some("BEGIN PRIVATE KEY"));
    assert_eq!(scan::secret_hit(b"public-proxy-marker AKIA123"), Some("AKIA"));
}

#[test]
fn manifest_and_handoff_round_trip() {
    let dir = std::env::temp_dir().join(format!("oo-kit-{}", stamp()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("oo"), b"abc").unwrap();
    let digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let manifest_text = format!("{digest}  oo\n");
    assert_eq!(manifest::verify_manifest(&dir, &manifest_text).unwrap(), 1);
    assert!(manifest::verify_manifest(&dir, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  oo\n").is_err());
    let facts = sample_facts(&dir, digest);
    fs::write(dir.join("facts.env"), &facts).unwrap();
    let parsed = manifest::read_facts(&facts).unwrap();
    assert_eq!(parsed.get("pi_version").unwrap(), PI_VERSION);
    let cjk = cmap::spans_json(&cmap::cjk_spans(&cmap::synthetic_format4(0x4E00, 0x4E00)).unwrap());
    let probe = r#"{"ok":true,"kind":"installation-probe","qa_complete":false}"#;
    let document = handoff::build(&facts, &cjk, probe, "oo-a406-eef831aa-pi-kit", "/var/lib/docker/volumes/example/_data").unwrap();
    let parsed = json::parse(document.trim()).unwrap();
    assert_eq!(parsed.get("consumer_uuid").and_then(json::Json::as_str), Some("eef831aa-dba4-4af2-a78b-541bc9e0a397"));
    assert_eq!(parsed.get("qa_complete").and_then(json::Json::as_bool), Some(false));
    assert_eq!(parsed.get("mcp").and_then(json::Json::as_bool), Some(false));
    assert!(parsed.get("commands").and_then(|value| value.get("rpc")).is_some());
    let _ = fs::remove_dir_all(&dir);
}

fn sample_facts(dir: &std::path::Path, digest: &str) -> String {
    let path = dir.join("oo");
    let path = path.display();
    format!(
        "\
source_commit=d858fa708c4c5102f79aee47659dc51cb0c46bde
pi_commit=7c10bd4337495ee613f2224843ecdf349b80d1df
pi_version=1.0.4
node_version=v22.23.3
image=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
profile=debug
bash_version=5.2.37
rg_version=14.1.1
pi_executable={path}
pi_executable_sha256={digest}
pi_runtime={path}
pi_runtime_sha256={digest}
oo_bin={path}
oo_bin_sha256={digest}
query_pg_bin={path}
query_pg_bin_sha256={digest}
bash_bin={path}
bash_bin_sha256={digest}
rg_bin={path}
rg_bin_sha256={digest}
static_library={path}
static_library_sha256={digest}
cjk_font={path}
cjk_font_sha256={digest}
"
    )
}

fn stamp() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos()
}
