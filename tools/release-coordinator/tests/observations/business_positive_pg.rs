//! Real execution and independent TLS identities on the owned local fixture only.
use crate::{ControlConn, Endpoint, PlanRequest};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub(crate) struct Fixture {
    pub(crate) endpoint: Endpoint,
    pub(crate) ca: Vec<u8>,
    pub(crate) password: String,
    pub(crate) bootstrap: String,
    pub(crate) actors: BTreeMap<String, String>,
    pub(crate) out: PathBuf,
    pub(crate) config: Value,
}
impl Fixture {
    pub(crate) fn load() -> Self {
        let v: Value = serde_json::from_slice(
            &std::fs::read(std::env::var("OO_BUSINESS_GUARD_CONFIG").unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(v["host"], "127.0.0.1");
        assert!(
            v["database"]
                .as_str()
                .unwrap()
                .starts_with("oo_business_guard_")
        );
        let read = |path: &Value| std::fs::read_to_string(path.as_str().unwrap()).unwrap();
        Self {
            config: v.clone(),
            endpoint: Endpoint {
                host: "127.0.0.1".into(),
                port: v["port"].as_u64().unwrap().try_into().unwrap(),
                database: v["database"].as_str().unwrap().into(),
                pack_ceilings: None,
            },
            ca: std::fs::read(v["ca_file"].as_str().unwrap()).unwrap(),
            password: read(&v["password_file"]),
            bootstrap: read(&v["bootstrap_password_file"]),
            actors: v["actor_password_files"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, p)| (k.clone(), read(p)))
                .collect(),
            out: PathBuf::from(std::env::var("OO_RELEASE_GATE_EVIDENCE_DIR").unwrap()),
        }
    }
    pub(crate) async fn cli(&self, command: &str, input: Value, policy: Option<&str>) -> crate::CommandExit {
        let mut config = json!({"host":self.endpoint.host,"port":self.endpoint.port,"database":self.endpoint.database,
            "user":"oo_release_admin","password_file":self.config["password_file"],"ca_file":self.config["ca_file"]});
        if command == "business-review" {
            config["user"] = json!("biz_reviewer");
            config["password_file"] = self.config["actor_password_files"]["reviewer"].clone();
        }
        if command == "business-install" {
            let pin = policy.unwrap();
            config["business_policy"] =
                json!({"path":self.out.join(format!("policy-{pin}.json")),"sha256":pin});
        }
        if command == "business-execute" {
            let mut actors = serde_json::Map::new();
            for kind in ["executor", "oracle", "evaluator"] {
                let mut a = config.clone();
                a["user"] = json!(format!("biz_{kind}"));
                a["password_file"] = self.config["actor_password_files"][kind].clone();
                actors.insert(kind.into(), a);
            }
            config["business_actors"] = json!(actors);
        }
        // Config stays in the runner's private temporary directory; it is not a deliverable.
        let path = Path::new(self.config["password_file"].as_str().unwrap())
            .parent()
            .unwrap()
            .join(format!("{command}.json"));
        std::fs::write(&path, crate::canon::canonical(&config)).unwrap();
        let args = vec![
            command.into(),
            "--config".into(),
            path.display().to_string(),
        ];
        use std::io::Write;
        let mut child = std::process::Command::new(std::env::var("OO_BUSINESS_CLI_BIN").unwrap())
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(crate::canon::canonical(&input).as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        crate::CommandExit {
            code: output.status.code().unwrap_or(1),
            stdout: output.stdout,
            stderr: output.stderr,
        }
    }
    pub(crate) fn control(&self) -> ControlConn<'_> {
        ControlConn {
            endpoint: &self.endpoint,
            user: "oo_release_admin",
            password: self.password.trim(),
            ca_pem: &self.ca,
        }
    }
    pub(crate) fn actor(&self, kind: &str) -> ControlConn<'_> {
        let user = match kind {
            "executor" => "biz_executor",
            "oracle" => "biz_oracle",
            "evaluator" => "biz_evaluator",
            "reviewer" => "biz_reviewer",
            _ => panic!("fixture actor"),
        };
        ControlConn {
            endpoint: &self.endpoint,
            user,
            password: self.actors[kind].trim(),
            ca_pem: &self.ca,
        }
    }
    pub(crate) fn plan<'a>(&'a self, source: &'a str) -> PlanRequest<'a> {
        PlanRequest {
            endpoint: &self.endpoint,
            user: "oo_release_admin",
            password: self.password.trim(),
            ca_pem: &self.ca,
            project_id: "shop",
            candidate_id: "positive",
            source_hash: source,
            scope: "test_scope",
            expected_version: None,
        }
    }
    pub(crate) async fn install(&self, p: &Value) -> String {
        let raw = crate::canon::canonical(p);
        let pin = crate::sha256_hex(raw.as_bytes());
        let path = self.out.join(format!("policy-{pin}.json"));
        std::fs::write(&path, &raw).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let receipt = self
            .cli(
                "business-install",
                json!({"protocol":"oo.release-coordinator/business-install/v1"}),
                Some(&pin),
            )
            .await;
        assert_eq!(
            receipt.code,
            0,
            "{}",
            String::from_utf8_lossy(&receipt.stderr)
        );
        pin
    }
    pub(crate) async fn execute_review(&self, pin: &str, run: &str, log: &mut Vec<Value>) {
        let executed = self
            .cli(
                "business-execute",
                json!({"protocol":"oo.release-coordinator/business-execute/v1",
            "policy_sha256":pin,"run_id":run}),
                None,
            )
            .await;
        assert_eq!(
            executed.code,
            0,
            "{}",
            String::from_utf8_lossy(&executed.stderr)
        );
        let execution = crate::Receipt {
            code: 0,
            body: serde_json::from_slice(&executed.stdout).unwrap(),
        };
        let expected = execution.body["execution_bundle_sha256"].as_str().unwrap();
        assert!(
            crate::review_business_checks(&self.actor("executor"), pin, run, expected)
                .await
                .is_err()
        );
        assert!(
            crate::review_business_checks(&self.actor("reviewer"), pin, run, &"0".repeat(64))
                .await
                .is_err()
        );
        let review = self
            .cli(
                "business-review",
                json!({"protocol":"oo.release-coordinator/business-review/v1",
            "policy_sha256":pin,"run_id":run,"execution_bundle_sha256":expected}),
                None,
            )
            .await;
        assert_eq!(
            review.code,
            0,
            "{}",
            String::from_utf8_lossy(&review.stderr)
        );
        let reviewed = crate::Receipt {
            code: 0,
            body: serde_json::from_slice(&review.stdout).unwrap(),
        };
        log.push(json!({"operation":"execute","exit":execution.code,"receipt":execution.body}));
        log.push(json!({"operation":"review","exit":reviewed.code,"receipt":reviewed.body}));
    }
}
pub(crate) fn business_gates(receipt: &crate::Receipt, admitted: bool) {
    for id in ["G5a", "G5b", "G6", "G10", "G11"] {
        let gate = receipt.body["gates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["gate_id"] == id)
            .unwrap();
        assert_eq!(gate["test_admissible"], admitted, "{id}: {}", receipt.body);
        assert_eq!(gate["production_admissible"], false);
        if admitted {
            let evidence = gate["business_evidence"].as_array().unwrap();
            assert!(!evidence.is_empty());
            for receipt in evidence {
                assert_eq!(
                    receipt["expected_result_digest"],
                    receipt["observed_result_digest"]
                );
                assert_eq!(receipt["test_admissible"], true);
                assert_eq!(receipt["production_admissible"], false);
                assert_eq!(receipt["business_owner_signed"], false);
                assert!(
                    receipt["authenticated_receipt_reference"]["linked_receipts"]
                        .as_object()
                        .unwrap()
                        .len()
                        == 4
                );
            }
        }
    }
}
pub(crate) fn write_new(path: &Path, value: &Value) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    f.write_all(crate::canon::canonical(value).as_bytes())
        .unwrap();
}

#[tokio::test]
#[ignore = "run tests/run_business_guard_fixture.py --positive with a fresh private cluster"]
async fn actual_test_authority_executes_reviews_and_rechecks_cas() {
    let f = Fixture::load();
    let root = crate::store::open(&f.endpoint, "postgres", f.bootstrap.trim(), &f.ca)
        .await
        .unwrap();
    let control = crate::store::open(&f.endpoint, "oo_release_admin", f.password.trim(), &f.ca)
        .await
        .unwrap();
    crate::store::require_tls(&control.client).await.unwrap();
    assert!(
        control
            .client
            .query_one("SELECT to_regnamespace('oo_release_control')::text", &[])
            .await
            .unwrap()
            .get::<_, Option<String>>(0)
            .is_none()
    );
    let covers = crate::GATE_IDS
        .iter()
        .map(|g| (*g).to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        crate::install(&crate::InstallRequest {
            endpoint: &f.endpoint,
            user: "oo_release_admin",
            password: f.password.trim(),
            ca_pem: &f.ca,
            project_id: "shop",
            authority_id: "controlled-test-authority",
            covers: &covers
        })
        .await
        .unwrap()
        .code,
        0
    );
    let (mut effective, visibility) = crate::business_guard_pg::business_candidate();
    effective["metrics"][0]["grain"] = json!(["record"]);
    effective["examples"][0]["last_checked"]["result_hash"] =
        json!(crate::canon::canonical_sha(&json!({"scalar":"3"})));
    let staged = crate::stage(&crate::StageRequest {
        endpoint: &f.endpoint,
        user: "oo_release_admin",
        password: f.password.trim(),
        ca_pem: &f.ca,
        project_id: "shop",
        candidate_id: "positive",
        version: "positive-1",
        base_version: None,
        effective: &effective,
        visibility: &visibility,
        policy_id: "pol-shop",
        policy_version: "1",
        knowledge_id: "know-shop",
        knowledge_version: "1",
        catalog_version: "cat-1",
    })
    .await
    .unwrap();
    assert_eq!(staged.code, 0, "{}", staged.body);
    let source = staged.body["source_hash"].as_str().unwrap().to_owned();
    let mut log = vec![json!({"operation":"stage","exit":staged.code,"receipt":staged.body})];
    for id in crate::GATE_IDS {
        if ["G5a", "G5b", "G6", "G10", "G11"].contains(id) {
            continue;
        }
        let issuer = if *id == "G8" {
            "component_authority"
        } else {
            "test_issuer"
        };
        let recorded = crate::gate_record(&crate::GateRequest {
            endpoint: &f.endpoint,
            user: "oo_release_admin",
            password: f.password.trim(),
            ca_pem: &f.ca,
            project_id: "shop",
            candidate_id: "positive",
            source_hash: &source,
            gate_id: id,
            issuer_class: issuer,
            status: "pass",
            blocking: false,
            report_type: "test_issuer",
            authority_statement_sha256: None,
        })
        .await
        .unwrap();
        assert_eq!(recorded.code, 0, "{}", recorded.body);
    }
    // Existing test-scope barriers, not a claim of a customer fence or standby.
    for (kind, barrier) in [("fence", "closed"), ("replica", "missing")] {
        assert_eq!(
            crate::checkpoint(&crate::CheckpointRequest {
                endpoint: &f.endpoint,
                user: "oo_release_admin",
                password: f.password.trim(),
                ca_pem: &f.ca,
                project_id: "shop",
                candidate_id: "positive",
                kind,
                source_hash: &source,
                epoch: "fixture-epoch",
                barrier,
                sessions_terminated: false,
                issuer_class: "test_issuer",
                authority_statement_sha256: None
            })
            .await
            .unwrap()
            .code,
            0
        );
    }
    let time: i64 = control
        .client
        .query_one(
            "SELECT floor(extract(epoch FROM clock_timestamp()))::bigint",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    let target = json!({"aggregate":"count","column":"id","relation":"public.customer"});
    let oracle = json!({"aggregate":"sum","column":"n","relation":"public.independent_counts"});
    let mut policy = json!({"protocol":"oo.release-coordinator/business-policy/v1","scope":"test_scope",
        "execution_capability_version":crate::business_execution_capability(),
        "project_id":"shop","candidate_id":"positive","candidate_digest":source,"policy_revision":"one",
        "visibility_sha256":crate::canon::canonical_sha(&visibility),"effective_sha256":crate::canon::canonical_sha(&effective),
        "role":"oo_r_clerk","actors":{"executor":"biz_executor","oracle":"biz_oracle","evaluator":"biz_evaluator","reviewer":"biz_reviewer"},
        "relations":[{"name":"public.customer","role":"oo_r_clerk","columns":["id"]},
                     {"name":"public.independent_counts","role":"biz_oracle","columns":["n"]}],
        "generation_relation":"public.capture_generation",
        "checks":[{"asset":"metric:customer_count","query":target,"oracle":oracle},
                  {"asset":"example:customer-count","query":target,"oracle":oracle},
                  {"asset":"eval:business-fixture","query":target,"oracle":oracle}],
        "expires_at":time+600,"receipt_ttl_seconds":120});
    let pin = f.install(&policy).await;
    let file = f.out.join(format!("policy-{pin}.json"));
    assert!(
        crate::install_business_policy(&f.control(), &file, &"0".repeat(64))
            .await
            .is_err()
    );
    assert!(
        control
            .client
            .batch_execute("TRUNCATE oo_release_control.business_receipt")
            .await
            .is_err()
    );
    assert!(
        crate::execute_business_checks(
            &f.control(),
            &f.actor("reviewer"),
            &f.actor("oracle"),
            &f.actor("evaluator"),
            &pin,
            "wrong-issuer"
        )
        .await
        .is_err()
    );
    let absent = crate::plan(&f.plan(&source)).await.unwrap();
    assert_eq!(absent.code, 2);
    business_gates(&absent, false);
    log.push(json!({"operation":"no_evidence","exit":absent.code,"receipt":absent.body}));
    root.client
        .execute("UPDATE public.independent_counts SET n=100", &[])
        .await
        .unwrap();
    let wrong = crate::execute_business_checks(
        &f.control(),
        &f.actor("executor"),
        &f.actor("oracle"),
        &f.actor("evaluator"),
        &pin,
        "mismatch",
    )
    .await
    .unwrap();
    assert!(
        crate::review_business_checks(
            &f.actor("reviewer"),
            &pin,
            "mismatch",
            wrong.body["execution_bundle_sha256"].as_str().unwrap()
        )
        .await
        .is_err()
    );
    business_gates(&crate::status(&f.plan(&source)).await.unwrap(), false);
    log.push(json!({"operation":"independent_oracle_mismatch","review_refused":true,"receipt":wrong.body}));
    root.client.batch_execute("DELETE FROM public.independent_counts; INSERT INTO public.independent_counts VALUES(1),(2)").await.unwrap();
    f.execute_review(&pin, "good", &mut log).await;
    let plan = crate::plan(&f.plan(&source)).await.unwrap();
    assert_eq!(plan.code, 0, "{}", plan.body);
    business_gates(&plan, true);
    assert_eq!(plan.body["production_ready_certificate"], false);
    log.push(json!({"operation":"positive_plan","exit":plan.code,"receipt":plan.body}));
    // Immutable ledger and session_user prevent rewriting or impersonating a reviewer.
    let reviewer = crate::store::open(
        &f.endpoint,
        "biz_reviewer",
        f.actors["reviewer"].trim(),
        &f.ca,
    )
    .await
    .unwrap();
    assert!(
        reviewer
            .client
            .execute(
                "UPDATE oo_release_control.business_receipt SET body='{}' WHERE receipt_id='good'",
                &[]
            )
            .await
            .is_err()
    );
    assert!(
        control
            .client
            .execute(
                "DELETE FROM oo_release_control.business_receipt WHERE receipt_id='good'",
                &[]
            )
            .await
            .is_err()
    );
    assert!(control.client.execute("INSERT INTO oo_release_control.business_receipt(policy_sha,kind,receipt_id,actor,actor_oid,body,body_sha) \
        VALUES($1,'reviewer','forged','biz_reviewer',0,'{}',$2)",&[&pin,&"a".repeat(64)]).await.is_err());
    assert_eq!(
        crate::enter_applying(&f.plan(&source)).await.unwrap().code,
        0
    );
    root.client
        .execute("INSERT INTO public.customer VALUES(4,'fixture')", &[])
        .await
        .unwrap();
    let changed = crate::commit_activation(&f.plan(&source)).await.unwrap();
    assert_eq!(changed.code, 2);
    log.push(
        json!({"operation":"data_changed_after_plan","exit":changed.code,"receipt":changed.body}),
    );
    root.client
        .execute("DELETE FROM public.customer WHERE id=4", &[])
        .await
        .unwrap();
    // Contents restored, but the transactional capture generation has advanced.
    business_gates(&crate::status(&f.plan(&source)).await.unwrap(), false);
    f.execute_review(&pin, "new-generation", &mut log).await;
    assert_eq!(crate::plan(&f.plan(&source)).await.unwrap().code, 0);
    root.client
        .batch_execute("REVOKE oo_r_clerk FROM biz_evaluator")
        .await
        .unwrap();
    business_gates(&crate::status(&f.plan(&source)).await.unwrap(), false);
    root.client
        .batch_execute("GRANT oo_r_clerk TO biz_evaluator")
        .await
        .unwrap();
    business_gates(&crate::status(&f.plan(&source)).await.unwrap(), false);
    assert_eq!(
        crate::revoke_business_policy(&f.control(), &pin)
            .await
            .unwrap()
            .code,
        0
    );
    policy["policy_revision"] = json!("two");
    let pin2 = f.install(&policy).await;
    f.execute_review(&pin2, "revocation-cas", &mut log).await;
    assert_eq!(crate::plan(&f.plan(&source)).await.unwrap().code, 0);
    assert_eq!(
        crate::enter_applying(&f.plan(&source)).await.unwrap().code,
        0
    );
    crate::revoke_business_policy(&f.control(), &pin2)
        .await
        .unwrap();
    let revoked = crate::commit_activation(&f.plan(&source)).await.unwrap();
    assert_eq!(revoked.code, 2);
    log.push(json!({"operation":"revoked_after_plan","exit":revoked.code,"receipt":revoked.body}));
    policy["policy_revision"] = json!("three");
    policy["receipt_ttl_seconds"] = json!(4);
    let pin3 = f.install(&policy).await;
    f.execute_review(&pin3, "expiry-cas", &mut log).await;
    assert_eq!(crate::plan(&f.plan(&source)).await.unwrap().code, 0);
    assert_eq!(
        crate::enter_applying(&f.plan(&source)).await.unwrap().code,
        0
    );
    root.client
        .query_one("SELECT pg_sleep(5)", &[])
        .await
        .unwrap();
    let expired = crate::commit_activation(&f.plan(&source)).await.unwrap();
    assert_eq!(expired.code, 2);
    log.push(json!({"operation":"expired_after_plan","exit":expired.code,"receipt":expired.body}));
    crate::revoke_business_policy(&f.control(), &pin3)
        .await
        .unwrap();
    policy["policy_revision"] = json!("four");
    policy["receipt_ttl_seconds"] = json!(120);
    let pin4 = f.install(&policy).await;
    f.execute_review(&pin4, "activate", &mut log).await;
    let production = crate::plan(&PlanRequest {
        scope: "production",
        ..f.plan(&source)
    })
    .await
    .unwrap();
    assert_eq!(production.code, 2);
    assert_eq!(production.body["production_ready_certificate"], false);
    let ready = crate::plan(&f.plan(&source)).await.unwrap();
    assert_eq!(ready.code, 0);
    business_gates(&ready, true);
    // The same resolver is used by the maintenance preflight, without changing required assets.
    let mut session = crate::store::open(&f.endpoint, "oo_release_admin", f.password.trim(), &f.ca)
        .await
        .unwrap();
    let proof = crate::business_positive::preflight(
        &mut session,
        "positive",
        &source,
        &effective,
        &visibility,
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        ["G5a", "G5b", "G6", "G10", "G11"]
            .iter()
            .all(|g| proof.covers(g))
    );
    let active = crate::activate(&f.plan(&source)).await.unwrap();
    assert_eq!(active.code, 0, "{}", active.body);
    assert_eq!(
        active.body["certificate_class"],
        "test_scope_not_production"
    );
    assert_eq!(active.body["owner_authenticated"], false);
    assert_eq!(active.body["production_ready_certificate"], false);
    log.push(
        json!({"operation":"production_refused","exit":production.code,"receipt":production.body}),
    );
    log.push(
        json!({"operation":"positive_test_activation","exit":active.code,"receipt":active.body}),
    );
    let pointer: Option<String> = control
        .client
        .query_one(
            "SELECT candidate_id FROM oo_release_control.active_pointer WHERE singleton",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(pointer.as_deref(), Some("positive"));
    let rows=control.client.query("SELECT policy_sha,kind,receipt_id,actor,actor_oid::bigint,body,body_sha,recorded_at::text FROM oo_release_control.business_receipt ORDER BY policy_sha,receipt_id,kind",&[]).await.unwrap();
    let receipts:Vec<Value>=rows.iter().map(|r|json!({"policy_sha":r.get::<_,String>(0),"kind":r.get::<_,String>(1),
        "receipt_id":r.get::<_,String>(2),"authenticated_actor":r.get::<_,String>(3),"actor_oid":r.get::<_,i64>(4),
        "body":serde_json::from_str::<Value>(&r.get::<_,String>(5)).unwrap(),"body_sha":r.get::<_,String>(6),
        "recorded_at":r.get::<_,String>(7)})).collect();
    write_new(
        &f.out.join("positive-chain.json"),
        &json!({"scope":"test_scope","actual_sql_executed":true,
        "actual_tls_issuer_logins":true,"customer_source_access":false,"business_owner_signed":false,
        "production_admissible":false,"candidate_digest":source,"effective":effective,"visibility":visibility,
        "journal":log,"immutable_receipts":receipts,"pointer":pointer}),
    );
}
