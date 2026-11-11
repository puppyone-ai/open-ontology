//! Non-TTY screen 4: `oo ontology graph` on the logistics fixture.
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/adjudication.json"
);

fn oo(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oo"))
        .args(args)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oo-graph-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn text_lists_relations_with_cardinality_and_missing_evidence() {
    let text = stdout(&oo(&[
        "ontology",
        "graph",
        "--adjudication",
        FIXTURE,
        "--format",
        "text",
    ]));
    assert!(
        text.contains("订单     >─1 客户    via orders.customer_id"),
        "{text}"
    );
    assert!(text.contains("司机     1─1 车辆"), "{text}");
    assert!(
        text.contains(
            "运单     >┄1 仓库    via shipments.origin_wh          ✗ containment 0.71 < 0.98"
        ),
        "{text}"
    );
    // Distinct coverage below the threshold is named, with row coverage.
    assert!(text.contains("✗ distinct 0.52 < 0.9 (rows 0.64)"), "{text}");
    // Structural flaws: a scoped parent key and a normalized collision.
    assert!(
        text.contains("✗ scope valid_from required, fan-out ×3"),
        "{text}"
    );
    assert!(text.contains("✗ 2 keys collide after trim/lower"), "{text}");
    // Two edges from one child to the same parent are both listed.
    assert!(text.contains("via routes.origin_station_id"), "{text}");
    assert!(text.contains("via routes.dest_station_id"), "{text}");
    assert_eq!(text.lines().count(), 20, "header + 19 relations");
}

#[test]
fn ascii_is_deterministic_bounded_and_draws_rejections() {
    let a = stdout(&oo(&[
        "ontology",
        "graph",
        "--adjudication",
        FIXTURE,
        "--format",
        "ascii",
        "--width",
        "80",
    ]));
    let b = stdout(&oo(&[
        "ontology",
        "graph",
        "--adjudication",
        FIXTURE,
        "--format=ascii",
        "--width=80",
    ]));
    assert_eq!(a, b);
    assert!(
        a.contains("┄┄ 运单 → 仓库  ✗ containment 0.71 < 0.98"),
        "{a}"
    );
    assert!(
        a.contains("├N╮") && a.contains("├1╯"),
        "self reference: {a}"
    );
    assert!(a.contains("│ 运单事件 │"), "{a}");
    assert!(a.contains("── 线路 → 网点  as dest_station"), "{a}");
    // Hidden sensitive columns never show values; the graph has none at all.
    assert!(!a.contains("phone"));
    let focus = stdout(&oo(&[
        "ontology",
        "graph",
        "--adjudication",
        FIXTURE,
        "--focus",
        "运单",
    ]));
    assert!(focus.contains("# focus 运单: 7 of 13 entities"), "{focus}");
    assert!(!focus.contains("客户"), "one hop only: {focus}");
}

#[test]
fn mermaid_and_json() {
    let m = stdout(&oo(&[
        "ontology",
        "graph",
        "--adjudication",
        FIXTURE,
        "--format",
        "mermaid",
    ]));
    assert!(m.starts_with("flowchart LR"));
    assert!(
        m.contains("orders -->|\"N:1 customer_id\"| customers"),
        "{m}"
    );
    assert!(
        m.contains("shipments -.->|\"rejected: containment 0.71 < 0.98\"| warehouses"),
        "{m}"
    );
    let j: serde_json::Value = serde_json::from_str(&stdout(&oo(&[
        "ontology",
        "graph",
        "--adjudication",
        FIXTURE,
        "--format",
        "text",
        "--json",
    ])))
    .unwrap();
    assert_eq!(j["protocol"], "oo.ontology-graph/v1");
    assert_eq!(j["counts"]["hidden_columns"], 2);
    assert_eq!(j["counts"]["demoted_metrics"], 1);
    assert_eq!(j["counts"]["rejected_relations"], 4);
}

#[test]
fn confirm_summary_binds_overrides_and_never_overwrites() {
    let dir = temp("confirm");
    let adj = dir.join("adjudication.json");
    std::fs::copy(FIXTURE, &adj).unwrap();
    let base: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(FIXTURE).unwrap()).unwrap();
    let summary = base["summary_sha256"].as_str().unwrap();
    let adj_s = adj.to_str().unwrap();
    // Wrong hash and missing reason are refused; nothing is written.
    let bad = oo(&[
        "ontology",
        "graph",
        "--adjudication",
        adj_s,
        "--confirm-summary",
        &"0".repeat(64),
        "--reason",
        "x",
    ]);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("CONFIRM_SUMMARY_MISMATCH"));
    assert!(
        !oo(&[
            "ontology",
            "graph",
            "--adjudication",
            adj_s,
            "--confirm-summary",
            summary
        ])
        .status
        .success()
    );
    assert!(!dir.join("publish-confirm.json").exists());
    // Plain confirmation.
    let out = stdout(&oo(&[
        "ontology",
        "graph",
        "--adjudication",
        adj_s,
        "--confirm-summary",
        summary,
        "--reason",
        "first publish",
        "--json",
    ]));
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(report["summary_sha256"], summary);
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("publish-confirm.json")).unwrap())
            .unwrap();
    assert_eq!(doc["protocol"], "oo.publish-confirm/v1");
    assert_eq!(doc["overrides_sha256"], serde_json::Value::Null);
    assert!(
        !oo(&[
            "ontology",
            "graph",
            "--adjudication",
            adj_s,
            "--confirm-summary",
            summary,
            "--reason",
            "again"
        ])
        .status
        .success(),
        "never overwrites"
    );
    // With an override, the old summary no longer confirms; the new one does.
    let overrides = dir.join("overrides.json");
    std::fs::write(
        &overrides,
        serde_json::json!({"protocol":"oo.adjudication-override/v1","summary_sha256":summary,
            "items":{"relation/shipments.origin_wh":{"decision":"adopted","reason":"legacy codes migrated"}}})
        .to_string(),
    )
    .unwrap();
    let o = overrides.to_str().unwrap();
    let second = dir.join("second.json");
    let s2 = second.to_str().unwrap();
    assert!(
        !oo(&[
            "ontology",
            "graph",
            "--adjudication",
            adj_s,
            "--overrides",
            o,
            "--confirm-summary",
            summary,
            "--reason",
            "r",
            "--out",
            s2
        ])
        .status
        .success()
    );
    let shown: serde_json::Value = serde_json::from_str(&stdout(&oo(&[
        "ontology",
        "graph",
        "--adjudication",
        adj_s,
        "--overrides",
        o,
        "--format",
        "text",
        "--json",
    ])))
    .unwrap();
    let effective = shown["summary_sha256"].as_str().unwrap();
    assert_ne!(effective, summary);
    assert!(
        shown["text"]
            .as_str()
            .unwrap()
            .contains("运单     >─1 仓库")
    );
    stdout(&oo(&[
        "ontology",
        "graph",
        "--adjudication",
        adj_s,
        "--overrides",
        o,
        "--confirm-summary",
        effective,
        "--reason",
        "r",
        "--out",
        s2,
    ]));
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&second).unwrap()).unwrap();
    assert_eq!(doc["summary_sha256"], effective);
    assert!(doc["overrides_sha256"].is_string());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn curated_demo_rejects_selection_instead_of_returning_a_preview() {
    for flags in [vec!["--tables-recommended"], vec!["--tables", "lg.nope"]] {
        let mut args = vec!["init", "--demo", "--json"];
        args.extend(flags);
        let result = oo(&args);
        assert_eq!(result.status.code(), Some(2));
        let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(report["class"], "usage");
        assert!(report["protocol"].is_null());
    }
    assert!(Path::new(FIXTURE).exists());
}
