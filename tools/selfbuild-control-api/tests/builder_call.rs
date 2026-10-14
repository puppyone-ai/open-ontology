use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use selfbuild_control_api::builder_call::{self, InvokeSpec, ModelKind};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oo-71-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn write_exe(path: &Path, body: &str) {
    let mut file = OpenOptions::new().create(true).truncate(true).write(true).mode(0o755).open(path).expect("exe");
    file.write_all(body.as_bytes()).expect("exe bytes");
}

fn spec(dir: &Path, binary: &str, bearer: &str) -> InvokeSpec {
    InvokeSpec {
        binary: binary.to_owned(),
        bearer_file: bearer.to_owned(),
        config_path: dir.join("pg-builder.config.json"),
        seal_path: dir.join("seal.json"),
        cards_path: dir.join("cards.json"),
        approval_path: dir.join("card-approval.json"),
        cache_path: dir.join("cache"),
        generation_path: dir.join("generation.json"),
        catalog_path: dir.join("catalog.json"),
        curated_path: dir.join("curated.json"),
        reviewer_label: "reviewer".to_owned(),
        reviewer_at: "2026-10-12".to_owned(),
        model_cache_dir: dir.join("model-cache").display().to_string(),
        cancel: Arc::new(AtomicBool::new(false)),
    }
}

fn touch_inputs(dir: &Path) {
    for name in ["pg-builder.config.json", "seal.json", "catalog.json"] {
        fs::write(dir.join(name), b"{}\n").expect(name);
    }
    fs::write(dir.join("cards.json"), b"{\"evidence_hash\":\"abc\"}\n").expect("cards");
}

#[tokio::test]
async fn missing_bearer_does_not_spawn() {
    let dir = scratch("blocked");
    let marker = dir.join("spawned");
    let binary = dir.join("builder");
    write_exe(&binary, "#!/bin/sh\ntouch \"$OO_MARKER\"\nexit 0\n");
    touch_inputs(&dir);
    let result = builder_call::invoke(spec(&dir, &binary.display().to_string(), &dir.join("missing-bearer").display().to_string())).await;
    assert_eq!(result.kind, ModelKind::Blocked);
    assert!(!result.spawned);
    assert!(!marker.exists());
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn present_bearer_missing_binary_is_closed() {
    let dir = scratch("absent");
    let bearer = dir.join("bearer");
    fs::write(&bearer, b"unit-bearer-token\n").expect("bearer");
    touch_inputs(&dir);
    let missing = dir.join("no-such-builder");
    let result = builder_call::invoke(spec(&dir, &missing.display().to_string(), &bearer.display().to_string())).await;
    assert_eq!(result.kind, ModelKind::Failed { detail: "builder.absent" });
    assert!(!result.spawned);
    assert!(!dir.join("generation.json").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn present_bearer_without_seal_does_not_forge_success() {
    let dir = scratch("inputs");
    let bearer = dir.join("bearer");
    fs::write(&bearer, b"unit-bearer-token\n").expect("bearer");
    let binary = dir.join("builder");
    write_exe(&binary, "#!/bin/sh\nexit 0\n");
    fs::write(dir.join("pg-builder.config.json"), b"{}\n").expect("config");
    let result = builder_call::invoke(spec(&dir, &binary.display().to_string(), &bearer.display().to_string())).await;
    assert_eq!(result.kind, ModelKind::Failed { detail: "builder.inputs_absent" });
    assert!(!result.spawned);
    let _ = fs::remove_dir_all(&dir);
}

fn ok_script() -> &'static str {
    r#"#!/bin/sh
set -eu
cmd=$1
out=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--out" ]; then
    out=$2
    shift
  fi
  shift
done
case "$cmd" in
  approve|curated) printf '%s' '{}' > "$out" ;;
  generate) printf '%s' '{"outcome":"validated","llm_calls":0,"cache_hit":true}' > "$out" ;;
  *) exit 2 ;;
esac
exit 0
"#
}

#[tokio::test]
async fn script_double_parses_validated_and_imports_cache() {
    let dir = scratch("ok");
    let bearer = dir.join("bearer");
    fs::write(&bearer, b"unit-bearer-token\n").expect("bearer");
    let binary = dir.join("builder");
    write_exe(&binary, ok_script());
    touch_inputs(&dir);
    let cache_root = dir.join("model-cache");
    fs::create_dir_all(cache_root.join("pg-builder")).expect("cache");
    let cache_key = "a".repeat(64);
    fs::write(cache_root.join(format!("pg-builder/{cache_key}.json")), format!("{{\"outcome\":\"validated\",\"evidence_hash\":\"abc\",\"input_sha256\":\"{cache_key}\"}}\n")).expect("hit");
    let result = builder_call::invoke(spec(&dir, &binary.display().to_string(), &bearer.display().to_string())).await;
    match result.kind {
        ModelKind::Validated { llm_calls, cache_hit, .. } => {
            assert_eq!(llm_calls, 0);
            assert!(cache_hit);
        }
        other => panic!("expected validated, got {other:?}"),
    }
    assert!(result.spawned);
    assert!(dir.join(format!("cache/pg-builder/{cache_key}.json")).is_file());
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn structural_draft_from_builder_is_not_success() {
    let dir = scratch("draft");
    let bearer = dir.join("bearer");
    fs::write(&bearer, b"unit-bearer-token\n").expect("bearer");
    let binary = dir.join("builder");
    write_exe(
        &binary,
        r#"#!/bin/sh
set -eu
cmd=$1
out=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--out" ]; then out=$2; shift; fi
  shift
done
case "$cmd" in
  approve) printf '%s' '{}' > "$out" ;;
  generate) printf '%s' '{"outcome":"structural_draft","llm_calls":0}' > "$out" ;;
esac
exit 0
"#,
    );
    touch_inputs(&dir);
    let result = builder_call::invoke(spec(&dir, &binary.display().to_string(), &bearer.display().to_string())).await;
    assert_eq!(result.kind, ModelKind::Failed { detail: "generate.failed" });
    assert!(result.spawned);
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn bearer_echo_is_a_secret_leak_and_not_ready() {
    let dir = scratch("leak");
    let bearer = dir.join("bearer");
    fs::write(&bearer, b"unit-bearer-token\n").expect("bearer");
    let binary = dir.join("builder");
    write_exe(
        &binary,
        r#"#!/bin/sh
printf '%s\n' unit-bearer-token
exit 0
"#,
    );
    touch_inputs(&dir);
    let result = builder_call::invoke(spec(&dir, &binary.display().to_string(), &bearer.display().to_string())).await;
    assert_eq!(result.kind, ModelKind::Failed { detail: "approval.failed" });
    assert!(result.spawned);
    assert!(!dir.join("generation.json").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn aggregate_transport_calls_are_counted() {
    let dir = scratch("aggregate");
    let bearer = dir.join("bearer");
    fs::write(&bearer, b"unit-bearer-token\n").unwrap();
    let binary = dir.join("builder");
    let script = ok_script().replace("\"llm_calls\":0,\"cache_hit\":true", "\"calls\":[{\"transport\":{\"socket\":\"opened\",\"attempt\":2}},{\"transport\":{\"socket\":\"not_opened\",\"attempt\":9}}],\"cache\":\"hit\"");
    write_exe(&binary, &script);
    touch_inputs(&dir);
    let result = builder_call::invoke(spec(&dir, &binary.display().to_string(), &bearer.display().to_string())).await;
    match result.kind {
        ModelKind::Validated { llm_calls, cache_hit, .. } => { assert_eq!(llm_calls, 2); assert!(cache_hit); }
        other => panic!("expected validated, got {other:?}"),
    }
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn sealed_money_facts_are_forwarded_and_only_a_matching_retained_completion_is_copied() {
    let dir = scratch("money-facts");
    let bearer = dir.join("bearer");
    fs::write(&bearer, b"unit-bearer-token\n").unwrap();
    let binary = dir.join("builder");
    write_exe(&binary, r#"#!/bin/sh
set -eu
out=""
prev=""
for arg in "$@"; do
  if [ "$prev" = "--out" ]; then out=$arg; fi
  prev=$arg
done
printf '%s\n' "$@" > "$(dirname "$out")/argv.txt"
mkdir -p "$out"
printf '%s' '{"outcome":"packed","attempt":1,"cache":"miss"}' > "$out/result.json"
exit 0
"#);
    touch_inputs(&dir);
    let catalog = br#"{"protocol":"oo.pipeline.sealed-import-catalog/v1"}"#;
    fs::write(dir.join("catalog.json"), catalog).unwrap();
    let native = br#"{"dataset":"logistics.shipments","settings":{},"visibility":{}}"#;
    fs::write(dir.join("native-model.json"), native).unwrap();
    fs::write(dir.join("source-money-facts.json"), b"{\"protocol\":\"oo.pg-builder.source-money-facts/v1\"}\n").unwrap();
    let evidence = selfbuild_control_api::canonical::sha256_hex(catalog);
    let cache_root = dir.join("model-cache/pg-builder");
    fs::create_dir_all(&cache_root).unwrap();
    let retained = format!("{{\"protocol\":\"oo.pg-builder.retained-completion/v1\",\"evidence_hash\":\"{evidence}\",\"model_content\":\"raw\"}}");
    fs::write(cache_root.join("retained-completion.json"), &retained).unwrap();
    fs::write(cache_root.join("retained-envelope.private.json"), b"envelope-bytes").unwrap();
    let result = builder_call::invoke(spec(&dir, &binary.display().to_string(), &bearer.display().to_string())).await;
    assert!(matches!(result.kind, ModelKind::Validated { .. }));
    let argv = fs::read_to_string(dir.join("argv.txt")).unwrap();
    assert!(argv.contains("--source-contract"));
    assert!(argv.contains("source-money-facts.json"));
    assert!(!argv.contains("native-source-contract.json"));
    assert_eq!(fs::read(dir.join("native-model.json")).unwrap(), native);
    assert_eq!(fs::read(dir.join("cache/pg-builder/retained-completion.json")).unwrap(), retained.as_bytes());
    assert_eq!(fs::read(dir.join("cache/pg-builder/retained-envelope.private.json")).unwrap(), b"envelope-bytes");
    let blocked = scratch("money-validated");
    let bearer = blocked.join("bearer");
    fs::write(&bearer, b"unit-bearer-token\n").unwrap();
    let binary = blocked.join("builder");
    write_exe(&binary, "#!/bin/sh\nexit 0\n");
    touch_inputs(&blocked);
    fs::write(blocked.join("catalog.json"), catalog).unwrap();
    fs::write(blocked.join("native-model.json"), native).unwrap();
    let root = blocked.join("model-cache/pg-builder");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("retained-completion.json"), format!("{{\"protocol\":\"oo.pg-builder.retained-completion/v1\",\"outcome\":\"validated\",\"evidence_hash\":\"{evidence}\"}}")).unwrap();
    fs::write(root.join("retained-envelope.private.json"), b"envelope-bytes").unwrap();
    let _ = builder_call::invoke(spec(&blocked, &binary.display().to_string(), &bearer.display().to_string())).await;
    assert!(!blocked.join("cache/pg-builder/retained-completion.json").exists());
    assert!(!blocked.join("cache/pg-builder/retained-envelope.private.json").exists());
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&blocked);
}
