use std::fs;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use source_support::json::{self, Json};
use source_support::output::Outcome;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn path(relative: &str) -> String {
    root().join(relative).to_str().unwrap().to_string()
}

fn public_matrix() -> String {
    path("../../docs/database-support/support-matrix.yaml")
}

fn public_doc() -> String {
    path("../../docs/database-support/01-admission-and-support-matrix.md")
}

fn schema() -> String {
    path("../source-control-api/schema/source-register.request.schema.json")
}

fn evidence() -> String {
    path("examples/evidence")
}

fn go(args: &[&str]) -> Outcome {
    let mut owned = vec!["source-support".to_string()];
    owned.extend(args.iter().copied().map(str::to_string));
    source_support::execute(&owned)
}

fn expect(outcome: &Outcome, exit_code: i32, code: &str, reason: &str) {
    let text = json::stringify(&outcome.to_json());
    let parsed = json::parse(text.as_bytes()).expect("stdout must be one json object");
    assert_eq!(parsed.get("contract").and_then(Json::string), Some("oo.source-support/v1"), "{text}");
    assert_eq!(parsed.get("matrix_mutated").and_then(Json::bool), Some(false), "{text}");
    assert_eq!(parsed.get("parent_api_integration").and_then(Json::string), Some("pending"), "{text}");
    assert_eq!(outcome.exit_code, exit_code, "{text}");
    assert_eq!(outcome.code, code, "{text}");
    assert_eq!(outcome.reason, reason, "{text}");
    assert_eq!(outcome.ok, exit_code == 0, "{text}");
}

fn extra<'a>(outcome: &'a Outcome, key: &str) -> &'a Json {
    match outcome.extra.iter().find(|(name, _)| *name == key) {
        Some((_, value)) => value,
        None => panic!("missing {key}"),
    }
}

fn text_of(outcome: &Outcome) -> String {
    json::stringify(&outcome.to_json())
}

#[test]
fn real_matrix_parses_forty_nine_targets() {
    let text = fs::read_to_string(public_matrix()).unwrap();
    match source_support::matrix::load(&text) {
        Ok(matrix) => {
            assert_eq!(matrix.schema, "oo-support-matrix/v1");
            assert_eq!(matrix.targets.len(), 49);
            assert!(matrix.targets.iter().all(|target| {
                target.caps.values().all(|level| level != "verified" && level != "certified" && level != "experimental")
            }));
        }
        Err(_) => {
            let index = source_support::yaml::PARSE_INDEX.load(Ordering::Relaxed);
            let start = index.saturating_sub(80);
            let end = (index + 80).min(text.len());
            panic!("yaml at {index}: {:?}", &text[start..end]);
        }
    }
}

#[test]
fn real_matrix_check_and_supported_are_empty() {
    let matrix = public_matrix();
    let doc = public_doc();
    let check = go(&["check", "--matrix", &matrix, "--doc", &doc]);
    expect(&check, 0, "CHECKED", "ok");
    assert_eq!(extra(&check, "target_count").int(), Some(49));
    assert_eq!(extra(&check, "verified_count").int(), Some(0));
    let supported = go(&["supported", "--matrix", &matrix]);
    expect(&supported, 0, "SUPPORTED", "listed");
    assert_eq!(extra(&supported, "verified_count").int(), Some(0));
    assert_eq!(extra(&supported, "supported").array().map(|items| items.len()), Some(0));
    assert_eq!(extra(&supported, "experimental").array().map(|items| items.len()), Some(0));
}

#[test]
fn real_postgres_catalog_is_designed_not_production() {
    let matrix = public_matrix();
    let request = path("examples/requests/admit-pg-catalog.json");
    let schema = schema();
    let before = fs::read(&matrix).unwrap();
    let outcome = go(&["admit", "--matrix", &matrix, "--request", &request, "--schema", &schema]);
    expect(&outcome, 2, "CAPABILITY_UNVERIFIED", "designed");
    assert_eq!(extra(&outcome, "target_id").string(), Some("postgres-selfmanaged"));
    assert_eq!(extra(&outcome, "capability").string(), Some("catalog"));
    assert_eq!(extra(&outcome, "level").string(), Some("designed"));
    assert_eq!(extra(&outcome, "version_fit").string(), Some("interval"));
    assert_eq!(extra(&outcome, "existing_source_stopped").bool(), Some(false));
    assert_eq!(extra(&outcome, "source_control_registration").string(), Some("validated_not_submitted"));
    assert_eq!(extra(&outcome, "source_control_host_allowlist").string(), Some("not_evaluated"));
    assert_eq!(extra(&outcome, "secret_resolution").string(), Some("not_evaluated"));
    assert_eq!(extra(&outcome, "project_id").string(), Some("project"));
    assert_eq!(extra(&outcome, "source_ref").string(), Some("orders_src"));
    let text = text_of(&outcome);
    assert!(!text.contains("secret://"));
    assert!(!text.contains("db.internal"));
    assert!(!text.contains("reader"));
    assert_eq!(fs::read(&matrix).unwrap(), before);
}

#[test]
fn real_matrix_rejections_are_exact() {
    let matrix = public_matrix();
    let evidence = evidence();
    expect(
        &go(&["select", "--matrix", &matrix, "--request", &path("examples/requests/select-unknown-family.json")]),
        2,
        "CONNECTOR_UNSUPPORTED",
        "family_absent",
    );
    expect(
        &go(&["select", "--matrix", &matrix, "--request", &path("examples/requests/select-mysql-57.json")]),
        2,
        "TARGET_VERSION_UNVERIFIED",
        "version",
    );
    expect(
        &go(&["select", "--matrix", &matrix, "--request", &path("examples/requests/select-mysql-8036.json")]),
        2,
        "TARGET_VERSION_UNVERIFIED",
        "patch_unproven",
    );
    expect(
        &go(&[
            "select",
            "--matrix",
            &matrix,
            "--request",
            &path("examples/requests/select-patch-forged.json"),
            "--evidence",
            &evidence,
        ]),
        3,
        "FORGED_EVIDENCE",
        "sha256",
    );
    expect(
        &go(&["select", "--matrix", &matrix, "--request", &path("examples/requests/select-explicit-cdc.json")]),
        2,
        "CAPABILITY_UNVERIFIED",
        "designed",
    );
    expect(
        &go(&["select", "--matrix", &matrix, "--request", &path("examples/requests/select-sync-auto.json")]),
        2,
        "CAPABILITY_UNVERIFIED",
        "sync_auto_empty",
    );
    let driver = go(&["select", "--matrix", &matrix, "--request", &path("examples/requests/select-driver-missing.json")]);
    expect(&driver, 2, "DRIVER_MISSING", "driver");
    assert_eq!(extra(&driver, "target_id").string(), Some("oracle-19c-selfmanaged"));
    let compat = go(&["select", "--matrix", &matrix, "--request", &path("examples/requests/select-compat-unknown.json")]);
    expect(&compat, 2, "COMPAT_MODE_UNKNOWN", "compat_mode");
    assert_eq!(extra(&compat, "target_id").string(), Some("dm8"));
    let known = go(&["select", "--matrix", &matrix, "--request", &path("examples/requests/select-compat-known.json")]);
    expect(&known, 2, "CONNECTOR_UNSUPPORTED", "candidate");
    assert_eq!(extra(&known, "target_id").string(), Some("dm8"));
    let schema = schema();
    expect(
        &go(&[
            "admit",
            "--matrix",
            &matrix,
            "--request",
            &path("examples/requests/admit-file-parquet.json"),
            "--schema",
            &schema,
        ]),
        2,
        "CONNECTOR_UNSUPPORTED",
        "source_control_not_claimed",
    );
    let csv = go(&[
        "admit",
        "--matrix",
        &matrix,
        "--request",
        &path("examples/requests/admit-file-csv.json"),
        "--schema",
        &schema,
    ]);
    expect(&csv, 2, "CAPABILITY_UNVERIFIED", "designed");
    assert_eq!(extra(&csv, "target_id").string(), Some("files-and-dumps"));
    assert_eq!(extra(&csv, "version_fit").string(), Some("absent"));
}

#[test]
fn password_and_closed_registration_do_not_echo_values() {
    let matrix = public_matrix();
    let schema = schema();
    let password = go(&[
        "admit",
        "--matrix",
        &matrix,
        "--request",
        &path("examples/requests/admit-password.json"),
        "--schema",
        &schema,
    ]);
    expect(&password, 1, "malformed", "credential");
    assert!(!text_of(&password).contains("fixture-marker-not-echoed"));
    let closed = go(&[
        "admit",
        "--matrix",
        &matrix,
        "--request",
        &path("examples/requests/admit-closed.json"),
        "--schema",
        &schema,
    ]);
    expect(&closed, 1, "closed_input", "strategy");
}

#[test]
fn fixture_admits_verified_experimental_and_certified_only_with_evidence() {
    let matrix = path("examples/fixture-positive.yaml");
    let evidence = evidence();
    let schema = schema();
    let before = fs::read(&matrix).unwrap();
    let verified = go(&[
        "admit",
        "--matrix",
        &matrix,
        "--request",
        &path("examples/requests/admit-pg-catalog.json"),
        "--schema",
        &schema,
        "--evidence",
        &evidence,
    ]);
    expect(&verified, 0, "ADMITTED", "verified");
    assert_eq!(extra(&verified, "evidence_id").string(), Some("ev-pg-catalog"));
    assert_eq!(extra(&verified, "production_allowed").bool(), Some(true));
    assert_eq!(extra(&verified, "version_fit").string(), Some("exact"));
    assert!(!text_of(&verified).contains("secret://"));
    assert!(!text_of(&verified).contains("db.internal"));
    let certified = go(&[
        "admit",
        "--matrix",
        &matrix,
        "--request",
        &path("examples/requests/admit-certified.json"),
        "--schema",
        &schema,
        "--evidence",
        &evidence,
    ]);
    expect(&certified, 0, "ADMITTED", "certified");
    assert_eq!(extra(&certified, "evidence_id").string(), Some("ev-pg-certified"));
    let experimental = go(&[
        "admit",
        "--matrix",
        &matrix,
        "--request",
        &path("examples/requests/admit-experimental.json"),
        "--schema",
        &schema,
    ]);
    expect(&experimental, 0, "ADMITTED_EXPERIMENTAL", "experimental");
    assert_eq!(extra(&experimental, "production_allowed").bool(), Some(false));
    assert_eq!(extra(&experimental, "version_fit").string(), Some("exact"));
    assert_eq!(extra(&experimental, "project_id").string(), Some("project"));
    assert_eq!(extra(&experimental, "source_ref").string(), Some("orders_src"));
    let warnings = extra(&experimental, "warnings").array().unwrap();
    let warnings: Vec<&str> = warnings.iter().filter_map(Json::string).collect();
    assert!(warnings.contains(&"experimental"));
    assert!(warnings.contains(&"experimental catalog limit"));
    let cdc = go(&["select", "--matrix", &matrix, "--request", &path("examples/requests/select-explicit-cdc.json")]);
    expect(&cdc, 2, "CAPABILITY_UNVERIFIED", "explicit_not_downgraded");
    assert_eq!(extra(&cdc, "capability").string(), Some("cdc"));
    let auto = go(&[
        "select",
        "--matrix",
        &matrix,
        "--request",
        &path("examples/requests/select-sync-auto.json"),
        "--evidence",
        &evidence,
    ]);
    expect(&auto, 0, "ADMITTED", "verified");
    assert_eq!(extra(&auto, "capability").string(), Some("catalog"));
    expect(
        &go(&["select", "--matrix", &matrix, "--request", &path("examples/requests/select-experimental-production.json")]),
        2,
        "CAPABILITY_UNVERIFIED",
        "experimental_production",
    );
    expect(
        &go(&["select", "--matrix", &matrix, "--request", &path("examples/requests/select-experimental-off.json")]),
        2,
        "CAPABILITY_UNVERIFIED",
        "experimental_not_enabled",
    );
    let listed = go(&["supported", "--matrix", &matrix]);
    expect(&listed, 0, "SUPPORTED", "listed");
    assert_eq!(extra(&listed, "verified_count").int(), Some(2));
    assert_eq!(extra(&listed, "experimental").array().map(|items| items.len()), Some(1));
    assert_eq!(fs::read(&matrix).unwrap(), before);
}

#[test]
fn patch_evidence_must_match_bytes() {
    let evidence = evidence();
    let patched = go(&[
        "select",
        "--matrix",
        &path("examples/fixture-patch.yaml"),
        "--request",
        &path("examples/requests/select-patch-ok.json"),
        "--evidence",
        &evidence,
    ]);
    expect(&patched, 0, "ADMITTED", "verified");
    assert_eq!(extra(&patched, "version_fit").string(), Some("patch"));
    assert_eq!(extra(&patched, "evidence_id").string(), Some("ev-mysql"));
}

#[test]
fn evidence_and_contract_faults_exit_three() {
    let doc = public_doc();
    let evidence = evidence();
    expect(
        &go(&["check", "--matrix", &path("examples/fixture-forged.yaml"), "--doc", &doc, "--evidence", &evidence]),
        3,
        "FORGED_EVIDENCE",
        "sha256",
    );
    expect(
        &go(&["check", "--matrix", &path("examples/fixture-handwritten.yaml"), "--doc", &doc, "--evidence", &evidence]),
        3,
        "HANDWRITTEN_GRADE",
        "generated",
    );
    expect(
        &go(&["check", "--matrix", &path("examples/fixture-missing.yaml"), "--doc", &doc, "--evidence", &evidence]),
        3,
        "MISSING_EVIDENCE",
        "evidence",
    );
    expect(
        &go(&["check", "--matrix", &path("examples/fixture-scope.yaml"), "--doc", &doc, "--evidence", &evidence]),
        3,
        "EVIDENCE_SCOPE",
        "managed_variant",
    );
    expect(
        &go(&[
            "check",
            "--matrix",
            &path("examples/fixture-certified-incomplete.yaml"),
            "--doc",
            &doc,
            "--evidence",
            &evidence,
        ]),
        3,
        "EVIDENCE_INCOMPLETE",
        "certified_customer",
    );
    expect(
        &go(&["check", "--matrix", &path("examples/fixture-unknown-cap.yaml"), "--doc", &doc]),
        3,
        "UNKNOWN_CAPABILITY",
        "capability",
    );
    expect(
        &go(&["check", "--matrix", &public_matrix(), "--doc", &path("examples/doc-drift.md")]),
        3,
        "DOC_DRIFT",
        "doc",
    );
    expect(
        &go(&["check", "--matrix", &path("examples/fixture-private.yaml"), "--doc", &doc]),
        3,
        "PRIVATE_SCOPE",
        "deployment_local",
    );
    let schema = schema();
    expect(
        &go(&[
            "admit",
            "--matrix",
            &path("examples/fixture-private.yaml"),
            "--request",
            &path("examples/requests/admit-pg-catalog.json"),
            "--schema",
            &schema,
        ]),
        2,
        "CONNECTOR_UNSUPPORTED",
        "deployment_local",
    );
    expect(
        &go(&[
            "admit",
            "--matrix",
            &path("examples/fixture-positive.yaml"),
            "--request",
            &path("examples/requests/admit-pg-catalog.json"),
            "--schema",
            &path("examples/register-schema-drift.json"),
            "--evidence",
            &evidence,
        ]),
        3,
        "CONTRACT_DRIFT",
        "register_schema",
    );
}

#[test]
fn downgrade_warns_and_leaves_the_source_running() {
    let matrix = path("examples/fixture-positive.yaml");
    let before = fs::read(&matrix).unwrap();
    let two = go(&["downgrade", "--matrix", &matrix, "--event", &path("examples/events/downgrade-two.json")]);
    expect(&two, 0, "DOWNGRADED", "two_consecutive_failures");
    assert_eq!(extra(&two, "applied").bool(), Some(true));
    assert_eq!(extra(&two, "to").string(), Some("experimental"));
    assert_eq!(extra(&two, "new_registration").string(), Some("reject"));
    assert_eq!(extra(&two, "existing_source_stopped").bool(), Some(false));
    let row = &extra(&two, "existing_sources").array().unwrap()[0];
    assert_eq!(row.get("state").and_then(Json::string), Some("running"));
    assert_eq!(row.get("stopped").and_then(Json::bool), Some(false));
    assert_eq!(row.get("capability_paused").and_then(Json::bool), Some(false));
    assert_eq!(row.get("data_retained").and_then(Json::bool), Some(true));
    let one = go(&["downgrade", "--matrix", &matrix, "--event", &path("examples/events/downgrade-one.json")]);
    expect(&one, 0, "DOWNGRADE_HELD", "nightly_failure_pending");
    assert_eq!(extra(&one, "applied").bool(), Some(false));
    assert_eq!(extra(&one, "capability").string(), Some("catalog"));
    let gap = go(&["downgrade", "--matrix", &matrix, "--event", &path("examples/events/downgrade-gap.json")]);
    expect(&gap, 0, "DOWNGRADE_HELD", "nightly_failure_pending");
    let security = go(&["downgrade", "--matrix", &matrix, "--event", &path("examples/events/downgrade-security.json")]);
    expect(&security, 0, "SOURCE_CAPABILITY_BLOCKED", "security");
    assert_eq!(extra(&security, "to").string(), Some("blocked"));
    assert_eq!(extra(&security, "new_registration").string(), Some("reject"));
    let blocked = &extra(&security, "existing_sources").array().unwrap()[0];
    assert_eq!(blocked.get("state").and_then(Json::string), Some("running"));
    assert_eq!(blocked.get("stopped").and_then(Json::bool), Some(false));
    assert_eq!(blocked.get("capability_paused").and_then(Json::bool), Some(true));
    assert_eq!(blocked.get("data_retained").and_then(Json::bool), Some(true));
    assert_eq!(fs::read(&matrix).unwrap(), before);
    let public = public_matrix();
    let public_before = fs::read(&public).unwrap();
    let designed = go(&["downgrade", "--matrix", &public, "--event", &path("examples/events/downgrade-designed.json")]);
    expect(&designed, 2, "CAPABILITY_UNVERIFIED", "not_verified");
    assert_eq!(extra(&designed, "applied").bool(), Some(false));
    assert_eq!(extra(&designed, "existing_source_stopped").bool(), Some(false));
    assert_eq!(fs::read(&public).unwrap(), public_before);
}

#[test]
fn review_calendar_boundaries_downgrade_without_writing_the_matrix() {
    let matrix = path("examples/repair/review/review-select.yaml");
    let before = fs::read(&matrix).unwrap();
    let public = public_matrix();
    let public_before = fs::read(&public).unwrap();
    let review = "examples/repair/review";
    let boundary = "examples/repair/boundary";

    let apr = go(&["downgrade", "--matrix", &matrix, "--event", &path(&format!("{review}/down-apr-may.json"))]);
    expect_downgraded(&apr);
    let feb = go(&["downgrade", "--matrix", &matrix, "--event", &path(&format!("{review}/down-feb-mar.json"))]);
    expect_downgraded(&feb);
    let march = go(&["downgrade", "--matrix", &matrix, "--event", &path(&format!("{review}/down-mar.json"))]);
    expect_downgraded(&march);
    let gap = go(&["downgrade", "--matrix", &matrix, "--event", &path(&format!("{review}/down-gap.json"))]);
    expect_held(&gap);
    let one = go(&["downgrade", "--matrix", &matrix, "--event", &path(&format!("{review}/down-one.json"))]);
    expect_held(&one);
    let designed = go(&["downgrade", "--matrix", &public, "--event", &path(&format!("{review}/down-public-designed.json"))]);
    expect(&designed, 2, "CAPABILITY_UNVERIFIED", "not_verified");
    assert_eq!(extra(&designed, "applied").bool(), Some(false));
    assert_eq!(extra(&designed, "existing_source_stopped").bool(), Some(false));
    let public_security = go(&["downgrade", "--matrix", &public, "--event", &path(&format!("{review}/down-public-security.json"))]);
    expect(&public_security, 0, "SOURCE_CAPABILITY_BLOCKED", "security");
    assert_eq!(extra(&public_security, "existing_source_stopped").bool(), Some(false));
    let fixture_security = go(&["downgrade", "--matrix", &matrix, "--event", &path(&format!("{review}/down-sec-fixture.json"))]);
    expect(&fixture_security, 0, "SOURCE_CAPABILITY_BLOCKED", "security");
    assert_eq!(extra(&fixture_security, "to").string(), Some("blocked"));
    assert_eq!(extra(&fixture_security, "existing_source_stopped").bool(), Some(false));

    for name in ["year-end.json", "leap-2000.json", "year-2100.json", "jun-jul.json", "jan-feb.json", "history.json"] {
        let outcome = go(&["downgrade", "--matrix", &matrix, "--event", &path(&format!("{boundary}/{name}"))]);
        expect_downgraded(&outcome);
    }
    for name in ["leap-2000-miss.json", "recovered.json"] {
        let outcome = go(&["downgrade", "--matrix", &matrix, "--event", &path(&format!("{boundary}/{name}"))]);
        expect_held(&outcome);
    }
    for name in ["invalid-2100-02-29.json", "invalid-2026-02-29.json", "invalid-apr-31.json", "reversed.json", "duplicate.json"] {
        let outcome = go(&["downgrade", "--matrix", &matrix, "--event", &path(&format!("{boundary}/{name}"))]);
        expect(&outcome, 1, "malformed", "day");
    }
    assert_eq!(fs::read(&matrix).unwrap(), before);
    assert_eq!(fs::read(&public).unwrap(), public_before);
}

fn expect_downgraded(outcome: &Outcome) {
    expect(outcome, 0, "DOWNGRADED", "two_consecutive_failures");
    assert_eq!(extra(outcome, "applied").bool(), Some(true));
    assert_eq!(extra(outcome, "from").string(), Some("verified"));
    assert_eq!(extra(outcome, "to").string(), Some("experimental"));
    assert_eq!(extra(outcome, "new_registration").string(), Some("reject"));
    assert_eq!(extra(outcome, "existing_source_stopped").bool(), Some(false));
    let row = &extra(outcome, "existing_sources").array().unwrap()[0];
    assert_eq!(row.get("state").and_then(Json::string), Some("running"));
    assert_eq!(row.get("stopped").and_then(Json::bool), Some(false));
}

fn expect_held(outcome: &Outcome) {
    expect(outcome, 0, "DOWNGRADE_HELD", "nightly_failure_pending");
    assert_eq!(extra(outcome, "applied").bool(), Some(false));
    assert_eq!(extra(outcome, "to").string(), Some("verified"));
    assert_eq!(extra(outcome, "new_registration").string(), Some("unchanged"));
    assert_eq!(extra(outcome, "existing_source_stopped").bool(), Some(false));
}
