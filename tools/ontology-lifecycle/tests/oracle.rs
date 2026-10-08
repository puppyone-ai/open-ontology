use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Command;
use std::thread;

use ontology_lifecycle::{assess_root, bind_loopback, hex, serve_once, sha256};

fn source_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn sha_file(path: &std::path::Path) -> String {
    hex(&sha256(&fs::read(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))))
}

#[test]
fn pinned_documents_match_the_phase_read() {
    let root = source_root();
    let docs = [
        ("docs/02-technology-stack.md", "c7bc07ca5937b425a757200a11f1a51f8b44478b9da4d25c057907f826e675b9"),
        ("docs/06-ontology-layer.md", "d224101642de6d0b36820ce9edf647b338ce1a0594c9cd3ac91fb9c5e9bdb749"),
        ("docs/08-agent-interface.md", "4006c07959d1366798b1325cae4b7743205f14202a23c71ed4dd4dd131a5ac34"),
        ("docs/11-deployment-and-operations.md", "1a60e23394e46db6e7dde4fff98022c331bb9a337f16c58e0ec588ef71a58f8f"),
        ("docs/12-configuration-reference.md", "ce0fd4080f6e7ae4ae8d36a4cfd82d9873034ae8dda50fd728c50dd0f45db810"),
        ("specs/open-ontology-v0.1-spec.md", "accac91210579da06387c94a28a6e71430a9c629cab9c6921c137d2e2f72fb91"),
        ("specs/open-ontology-v0.1-plan.md", "5d9d5e6585d7b220c0a4990d522b4ff771c5459de47eacf2551eaba79f8741e0"),
        ("specs/selfbuild-convergence-exit-plan.md", "746aaf9d5ee6bb19e976812a95c9a5469d15e717f5cd7825b96aa04b0be0fb2e"),
        ("tools/selfbuild-control-api/openapi/selfbuild-control-api.openapi.yaml", "4a7a155d801e47590f3c60927abf6035da4fc2ca118d318f7f8204330f149cf0"),
        ("tools/curated-git-apply/CONTRACT.md", "4b9f84f013554cee19dcb40209181b0276135a88d098c7261331dc6a7fab740c"),
        ("tools/release-control-api/openapi/release-control-api.openapi.yaml", "de281a35aa6f6a75e2a6c4cb8e1a9fb3d2f76dc20799db164c024d1bd4239946"),
        ("tools/release-coordinator/src/decide.rs", "439aa2112925934c7f448f98a220fb889b708cb27fd1c7aa214b21a4d75496f4"),
        ("tools/pg-builder/src/main.rs", "75b6366e89218e19849e4f1812f7d922512f4ea91f420a34d904347f28cb98ea"),
    ];
    for (rel, pin) in docs {
        let got = sha_file(&root.join(rel));
        assert_eq!(got, pin, "{rel} moved; re-read that source before continuing");
    }
}

#[test]
fn live_producers_do_not_close_the_chain() {
    let assessment = assess_root(&source_root()).expect("source root");
    assert!(!assessment.chain_closed);
    assert_eq!(assessment.execution, "not_delivered");
    assert!(!assessment.production_signed);
    assert!(!assessment.compiled_pack_is_publication);
    assert!(!assessment.parent_acceptance);
    assert_eq!(assessment.children_spawned, 0);
    for id in ["OC-01", "OC-02", "OC-03", "OC-04", "OC-05"] {
        let gap = assessment.gaps.iter().find(|gap| gap.id == id).expect(id);
        assert!(
            gap.status == "absent" || gap.status == "unpinned",
            "{id} status {}",
            gap.status
        );
        assert_ne!(gap.status, "executed");
    }
    let journey = assessment.gaps.iter().find(|gap| gap.id == "OC-06").expect("oc06");
    assert_eq!(journey.status, "blocked");
}

#[test]
fn http_refuses_a_success_claim_and_does_not_publish() {
    let listener = bind_loopback(0).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let root = source_root();
    let server = thread::spawn(move || serve_once(listener, &root).expect("serve"));
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).expect("timeout");
    let body = "{\"published\":true}";
    let request = format!(
        "POST /control/v1/lifecycle/runs HTTP/1.1\r\nHost: 127.0.0.1\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).expect("write");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read");
    assert!(response.starts_with("HTTP/1.1 400 "), "{response}");
    assert!(response.contains("\"chain_closed\":false"));
    assert!(response.contains("\"compiled_pack_is_publication\":false"));
    let assessment = server.join().expect("join");
    assert!(!assessment.chain_closed);
}

#[test]
fn http_assessment_is_a_conflict_not_an_active_pack() {
    let listener = bind_loopback(0).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let root = source_root();
    let server = thread::spawn(move || serve_once(listener, &root).expect("serve"));
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).expect("timeout");
    stream
        .write_all(b"POST /control/v1/lifecycle/runs HTTP/1.1\r\nHost: 127.0.0.1\r\ncontent-length: 2\r\n\r\n{}")
        .expect("write");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read");
    assert!(response.starts_with("HTTP/1.1 409 "), "{response}");
    assert!(response.contains("\"chain_closed\":false"));
    assert!(response.contains("\"execution\":\"not_delivered\""));
    assert!(response.contains("\"certificate_class\":\"not_issued\""));
    assert!(response.contains("OC-05"));
    let assessment = server.join().expect("join");
    assert!(!assessment.chain_closed);
    assert_eq!(assessment.children_spawned, 0);
}

#[test]
fn cli_run_keeps_the_guard_directory_empty() {
    let root = source_root();
    let guard = std::env::temp_dir().join(format!("oo-a403-guard-{}", std::process::id()));
    let _ = fs::remove_dir_all(&guard);
    fs::create_dir(&guard).expect("guard");
    let output = Command::new(env!("CARGO_BIN_EXE_ontology-lifecycle"))
        .args([
            "run",
            "--source-root",
            root.to_str().expect("utf8"),
            "--guard",
            guard.to_str().expect("utf8"),
        ])
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(2), "stderr {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert!(stdout.contains("\"chain_closed\":false"));
    assert!(stdout.contains("\"children_spawned\":0"));
    assert!(fs::read_dir(&guard).expect("read").next().is_none());
    fs::remove_dir(&guard).expect("cleanup");
}
