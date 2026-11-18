//! FM6 conformance (80 section 4, cards V-S6-02 and V-S6-03): the share model
//! `formal/quint/share_lifecycle.qnt` against the live engine (HTTP, real
//! publisher, real daemon-free share code). The scenarios are the model's
//! witnesses run on the engine; after every step the invariants of the model
//! are evaluated on the rows and responses:
//!
//! * `invCredentialDead`: a revoked, refreshed or expired share's refs credential is refused;
//! * `invRefreshAtomic`: a refresh leaves one new active share (`refreshed_from`), the old one is `refreshed`;
//! * `invPackBytesImmutable`: `pack_digest`, version, files of a share never change;
//! * `invUpdatesAppendOnly`: `share_update` seq 1..n without gap, earlier rows unchanged;
//! * `invDriftMarked` / `invChangeDelivered`: a used fact retracted at a version after the share's
//!   pack gives a block entry and `drifted`, for a share created before and after that version;
//! * `invCheckCodes`, `invCheckFailClosed`: exit codes 0 / 10 / 20 / 30;
//! * `invGroupSameVersion`, `invGroupAllOrNothing`: group members on one version, a failed group
//!   refresh changes nothing;
//! * `invRefreshPolicy`: `pinned` is never refreshed, `at-checkpoint` only upgrades on 0 or 10,
//!   a manual share only on an explicit refresh;
//! * `invExtendKeepsContent`: PATCH of `expires_at` moves the end date only.
//!
//! ```text
//! OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test conformance_share -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The ITF samples of the model (`formal/traces/share_lifecycle`) are checked for shape and
//! witness coverage in `fm6_traces_exercise_every_rule_the_scenarios_check`: a rule that no
//! scenario here checks fails that test.

mod common;
#[path = "itf_support/mod.rs"]
mod itf;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use common::engine::{Opts, TestEngine, key};
use common::fx::{DRIVE, Fx, golden};
use itf::*;
use oo_docs::cas::CasKind;
use serde_json::{Value, json};
use sqlx::Executor;

const FILES: [&str; 4] = [
    "People/林远舟.md",
    "Projects/星图看板.md",
    "Decisions/D-2026-02-星图采用自研引擎.md",
    "Decisions/D-2026-05-星图改用开源图表库.md",
];

fn gold(rel: &str) -> String {
    std::fs::read_to_string(golden().join("docs").join(rel)).unwrap()
}

async fn world(name: &str) -> (TestEngine, Fx) {
    let e = TestEngine::start(
        name,
        Opts {
            workers: true,
            stages: true,
            ..Opts::default()
        },
    )
    .await;
    let mut fx = Fx::attach(name, &e.cas).await;
    for f in FILES {
        let src = gold(f);
        fx.index_md(f, &src).await;
        fx.cas.put(CasKind::Orig, src.as_bytes()).unwrap();
    }
    publish_now(&mut fx).await;
    (e, fx)
}

async fn max_version(fx: &mut Fx) -> i64 {
    fx.scalar("SELECT coalesce(max(n), 0) FROM _oo_docs.ontology_version")
        .await
}

/// Wait until the engine's own pipeline has nothing queued or running (two looks, 200 ms apart). The engine
/// publishes by itself as parse and extract jobs finish; a test that publishes on top of that while jobs are
/// still in flight gets one version from its call and another from the pipeline, and the model counts one.
async fn drain_jobs(fx: &mut Fx) {
    let t = Instant::now();
    let mut quiet = 0;
    while quiet < 2 {
        let busy = fx
            .scalar("SELECT count(*) FROM _oo_docs.job WHERE state IN ('queued', 'running')")
            .await;
        quiet = if busy == 0 { quiet + 1 } else { 0 };
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "the pipeline did not go quiet"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn publish_now(fx: &mut Fx) {
    let before = max_version(fx).await;
    drain_jobs(fx).await;
    for _ in 0..100 {
        match fx.publish().await {
            oo_docs::publish::run::PublishOutcome::Busy => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            _ => break,
        }
    }
    wait_version(fx, before + 1).await;
    drain_jobs(fx).await;
}

async fn wait_version(fx: &mut Fx, n: i64) {
    let t = Instant::now();
    while max_version(fx).await < n {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "version {n} did not come"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn mk_share(e: &TestEngine, scope: &[&str], extra: Value) -> Value {
    let mut body = json!({"compile": {"drive_id": DRIVE, "scope": scope, "budget": 1200,
                                      "task": "星图看板的决定", "wait_ms": 0},
                          "mount": {"target_dir": "/tmp/oo-share-conformance"}});
    if let (Some(o), Some(x)) = (body.as_object_mut(), extra.as_object()) {
        for (k, v) in x {
            o.insert(k.clone(), v.clone());
        }
    }
    let r = e.post("/v1/docs/shares", &body).await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    r.json()
}

/// GET that waits out the engine's own rate limiter (`DOCS_ENGINE_RATE_LIMITED`).
async fn get_retry(e: &TestEngine, path: &str) -> common::http::Resp {
    loop {
        let r = e.get(path).await;
        if r.status != 429 {
            return r;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn post_retry(e: &TestEngine, path: &str, body: &Value) -> common::http::Resp {
    loop {
        let r = e.post(path, body).await;
        if r.status != 429 {
            return r;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn share(e: &TestEngine, sid: &str) -> Value {
    get_retry(e, &format!("/v1/docs/shares/{sid}")).await.json()
}

async fn updates(e: &TestEngine, sid: &str) -> Vec<Value> {
    get_retry(e, &format!("/v1/docs/shares/{sid}/updates"))
        .await
        .json()["items"]
        .as_array()
        .unwrap()
        .clone()
}

async fn check(e: &TestEngine, sid: &str, body: Value) -> Value {
    e.post(&format!("/v1/docs/shares/{sid}:check"), &body)
        .await
        .json()
}

async fn used_decision_fact(fx: &mut Fx, sid: &str) -> String {
    fx.text(&format!(
        "SELECT f.id::text FROM _oo_docs.share_fact_use u JOIN _oo_docs.fact f ON f.id = u.fact_id \
          WHERE u.share_id = '{sid}' AND f.predicate IN ('decides','attr.status') ORDER BY f.fact_key LIMIT 1"
    ))
    .await
    .remove(0)
}

/// A person marks a fact wrong (retract, no replacement) and the version comes.
async fn retract(e: &TestEngine, fx: &mut Fx, fid: &str) {
    let before = max_version(fx).await;
    let w = e
        .req(
            "POST",
            &format!("/v1/docs/facts/{fid}:correct"),
            &[("Idempotency-Key", &key()), ("If-Match", "\"r1\"")],
            Some(&json!({"action": "mark_wrong", "reason": "推翻"})),
        )
        .await;
    assert_eq!(w.status, 200, "{}", String::from_utf8_lossy(&w.body));
    wait_version(fx, before + 1).await;
}

async fn wait_state(e: &TestEngine, sid: &str, state: &str) -> Value {
    let t = Instant::now();
    loop {
        let s = share(e, sid).await;
        if s["state"] == state {
            return s;
        }
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "{sid} is {s}, wanted {state}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// `invUpdatesAppendOnly`: seq 1..n, never fewer, earlier rows unchanged since `before`.
fn append_only(before: &[Value], now: &[Value], who: &str) {
    for (i, u) in now.iter().enumerate() {
        assert_eq!(u["seq"], (i + 1) as i64, "{who}: seq gap");
    }
    assert!(now.len() >= before.len(), "{who}: entries disappeared");
    assert_eq!(
        &now[..before.len()],
        before,
        "{who}: an entry was rewritten"
    );
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn fm6_drift_delivery_check_codes_and_immutable_pack() {
    // witDrifted, witBlockCheck20, witCredRejected, invDriftMarked, invChangeDelivered,
    // invPackBytesImmutable, invUpdatesAppendOnly, invCheckCodes
    let (e, mut fx) = world("fm6_drift").await;
    let a = mk_share(&e, &["Decisions", "Projects"], json!({})).await;
    let sid = a["id"].as_str().unwrap().to_owned();
    let digest = a["pack_digest"].clone();
    let version = a["version"].clone();
    assert_eq!(check(&e, &sid, json!({})).await["exit_code"], 0);
    let before = updates(&e, &sid).await;
    let fid = used_decision_fact(&mut fx, &sid).await;
    retract(&e, &mut fx, &fid).await;
    let s = wait_state(&e, &sid, "drifted").await;
    // the pack of a share never changes; only UPDATES grows (invPackBytesImmutable)
    assert_eq!((&s["pack_digest"], &s["version"]), (&digest, &version));
    let now = updates(&e, &sid).await;
    append_only(&before, &now, "after retract");
    let block = now
        .iter()
        .find(|u| u["level"] == "block")
        .expect("block entry");
    // The block runs from the version the share was made on to the version that retracted the fact. Their
    // numbers are the engine's, not 1 and 2: under load the engine's own debounced publisher can publish the
    // part of the corpus that `world` has indexed so far before the test's publish, which makes two versions.
    let from = version.as_i64().expect("share version");
    let retracted_at: i64 = fx
        .scalar(&format!(
            "SELECT min(n) FROM _oo_docs.ontology_version WHERE n > {from} AND facts_retracted > 0"
        ))
        .await;
    assert!(retracted_at > from, "no version retracted the fact");
    assert_eq!(
        (block["from_version"].as_i64(), block["to_version"].as_i64()),
        (Some(from), Some(retracted_at))
    );
    // invCheckCodes: a block above `since` is 20, never 0 or 10; past the last seq it is 0
    let c = check(&e, &sid, json!({})).await;
    assert_eq!(c["exit_code"], 20);
    let last = now.last().unwrap()["seq"].as_i64().unwrap();
    let c = e
        .post(
            &format!("/v1/docs/shares/{sid}:check?since={last}"),
            &json!({}),
        )
        .await
        .json();
    assert_eq!(c["exit_code"], 0, "nothing above since: {c}");
    // another version: entries stay a prefix
    publish_extra(&e, &mut fx, "Decisions/新决定-fm6.md").await;
    let later = updates(&e, &sid).await;
    append_only(&now, &later, "after a second version");
    e.finish().await;
}

async fn publish_extra(_e: &TestEngine, fx: &mut Fx, path: &str) {
    let src = "---\ntype: decision\nstatus: confirmed\ndate: 2026-11-01\nowner: 林远舟\n---\n\n# 决定：新的决定\n\n林远舟决定采用新方案。\n";
    fx.index_md(path, src).await;
    publish_now(fx).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn fm6_late_group_join_receives_the_changes_it_missed() {
    // witLateJoinBackfill: invChangeDelivered for a share that joins a group at an older version
    let (e, mut fx) = world("fm6_late").await;
    let a = mk_share(&e, &["Decisions"], json!({"group": "TICKET-9"})).await;
    let sa = a["id"].as_str().unwrap().to_owned();
    let fid = used_decision_fact(&mut fx, &sa).await;
    retract(&e, &mut fx, &fid).await;
    wait_state(&e, &sa, "drifted").await;
    // another agent of the same ticket starts now: it gets the group's version (1), which still has the fact
    let late = mk_share(&e, &["Decisions"], json!({"group": "TICKET-9"})).await;
    assert_eq!(
        late["version"], a["version"],
        "an existing group pins the version"
    );
    let sl = late["id"].as_str().unwrap().to_owned();
    let used: i64 = fx
        .scalar(&format!(
            "SELECT count(*) FROM _oo_docs.share_fact_use WHERE share_id = '{sl}' AND fact_id = '{fid}'"
        ))
        .await;
    assert_eq!(
        used, 1,
        "the late member's pack holds the fact that was retracted at version 2"
    );
    // the change must reach it: a block entry, drifted, and the gate says 20
    let c = check(&e, &sl, json!({})).await;
    assert_eq!(
        c["exit_code"], 20,
        "the late member never heard that a fact it uses was retracted at version 2: {c}"
    );
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn fm6_refresh_policy_credentials_group_and_extension() {
    // invRefreshAtomic, invRefreshPolicy, invCredentialDead, invGroupSameVersion,
    // invGroupAllOrNothing, invExtendKeepsContent, witPinnedRefused, witAtCheckpoint
    let (e, mut fx) = world("fm6_policy").await;
    // manual: explicit refresh only, credential dies, one successor
    let m = mk_share(&e, &["Decisions"], json!({})).await;
    let sm = m["id"].as_str().unwrap().to_owned();
    let cred = {
        let sj: Value = serde_json::from_str(m["share_json"].as_str().unwrap()).unwrap();
        sj["refs"]["credential"].as_str().unwrap().to_owned()
    };
    publish_extra(&e, &mut fx, "Decisions/新决定-fm6b.md").await;
    assert_eq!(
        share(&e, &sm).await["state"],
        "active",
        "a manual share is never refreshed by itself"
    );
    let c = check(&e, &sm, json!({})).await;
    assert!(matches!(c["exit_code"].as_i64(), Some(0 | 10)));
    assert_eq!(
        share(&e, &sm).await["state"],
        "active",
        "check does not refresh a manual share"
    );
    // extension changes only the end date (G6: If-Match, at most 30 days from creation)
    let before = share(&e, &sm).await;
    let rev = before["revision"].as_i64().expect("share revision");
    let at = |days: i32| {
        format!(
            "SELECT to_char((created_at + interval '{days} days') AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') \
               FROM _oo_docs.share WHERE id = '{sm}'"
        )
    };
    let later = fx.text(&at(20)).await.remove(0);
    let beyond = fx.text(&at(31)).await.remove(0);
    let patch = |when: String| {
        let e = &e;
        let path = format!("/v1/docs/shares/{sm}");
        async move {
            e.req(
                "PATCH",
                &path,
                &[("If-Match", &format!("\"r{rev}\""))],
                Some(&json!({"expires_at": when})),
            )
            .await
        }
    };
    assert_eq!(
        patch(beyond).await.status,
        400,
        "more than 30 days after creation"
    );
    let p = patch(later.clone()).await;
    assert_eq!(p.status, 200, "{}", String::from_utf8_lossy(&p.body));
    let after = share(&e, &sm).await;
    assert_eq!(
        (&after["pack_digest"], &after["version"], &after["state"]),
        (&before["pack_digest"], &before["version"], &before["state"])
    );
    assert_ne!(after["expires_at"], before["expires_at"]);
    let r = e
        .post(&format!("/v1/docs/shares/{sm}:refresh"), &json!({}))
        .await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let new = r.json();
    assert_eq!(share(&e, &sm).await["state"], "refreshed");
    assert_eq!(new["state"], "active");
    assert_eq!(new["refreshed_from"], json!(sm));
    // the old credential is dead at once
    let auth = format!("Bearer {cred}");
    let denied = common::http::request(
        e.addr,
        "POST",
        &format!("/v1/docs/shares/{sm}:check"),
        &[("Authorization", &auth), ("Idempotency-Key", &key())],
        None,
    )
    .await;
    assert!(
        denied.status == 401 || denied.status == 410 || denied.status == 403,
        "credential of a refreshed share: {}",
        denied.status
    );
    assert_eq!(check(&e, &sm, json!({})).await["exit_code"], 30);
    // pinned: never refreshed
    let p = mk_share(&e, &["Decisions"], json!({"refresh_policy": "pinned"})).await;
    let r = e
        .post(
            &format!("/v1/docs/shares/{}:refresh", p["id"].as_str().unwrap()),
            &json!({}),
        )
        .await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (409, Some("DOCS_SHARE_PINNED"))
    );
    // at-checkpoint: a check that comes out 0 or 10 upgrades, and only then
    let ac = mk_share(
        &e,
        &["Decisions"],
        json!({"refresh_policy": "at-checkpoint"}),
    )
    .await;
    let sa = ac["id"].as_str().unwrap().to_owned();
    publish_extra(&e, &mut fx, "Decisions/新决定-fm6c.md").await;
    let c = check(&e, &sa, json!({})).await;
    assert!(matches!(c["exit_code"].as_i64(), Some(0 | 10)), "{c}");
    assert_eq!(
        share(&e, &sa).await["state"],
        "refreshed",
        "at-checkpoint upgrades on 0 or 10"
    );
    assert!(c["refreshed_to"].is_string());
    // revoke: terminal, 30, credential dead
    let live = mk_share(&e, &["Projects"], json!({})).await;
    let sl = live["id"].as_str().unwrap().to_owned();
    let rv = e
        .post(&format!("/v1/docs/shares/{sl}:revoke"), &json!({}))
        .await;
    assert_eq!(rv.status, 200);
    assert_eq!(share(&e, &sl).await["state"], "revoked");
    assert_eq!(check(&e, &sl, json!({})).await["exit_code"], 30);
    // expiry: `expires_at` in the past is 30 before and after the sweep
    let ex = mk_share(&e, &["Projects"], json!({})).await;
    let se = ex["id"].as_str().unwrap().to_owned();
    fx.admin
        .execute(sqlx::AssertSqlSafe(format!(
            "UPDATE _oo_docs.share SET expires_at = clock_timestamp() - interval '1 second' WHERE id = '{se}'"
        )))
        .await
        .unwrap();
    assert_eq!(check(&e, &se, json!({})).await["exit_code"], 30);
    e.finish().await;
}

#[test]
fn fm6_traces_exercise_every_rule_the_scenarios_check() {
    // the committed samples must contain each action and each check code the scenarios above stand for
    let traces = load_traces("share_lifecycle", "OO_FM6_TRACES_DIR");
    let mut acts = std::collections::BTreeSet::new();
    let mut codes = std::collections::BTreeSet::new();
    for (_, t) in &traces {
        for st in t["states"].as_array().unwrap() {
            if let Some(a) = action_of(st) {
                acts.insert(a.to_owned());
            }
            if text(&st["lastAct"]) == "check" {
                codes.insert(int(&st["chk"]["code"]));
            }
        }
    }
    for a in [
        "mount",
        "publishK",
        "refresh",
        "groupRefresh",
        "check",
        "revoke",
        "refsRead",
    ] {
        assert!(acts.contains(a), "no sample trace does `{a}`: {acts:?}");
    }
    assert!(
        codes.contains(&0) && codes.contains(&40),
        "check codes in samples: {codes:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// ITF trace replay (profile `stepReplay` of the model): publishes only add facts, so a share's `uses` do not matter.
// ---------------------------------------------------------------------------------------------

struct ReplayRig {
    e: TestEngine,
    fx: Fx,
    /// model slot -> share id
    sid: BTreeMap<i64, String>,
    /// model slot -> its refs credential (from the creation or refresh payload)
    cred: BTreeMap<i64, String>,
    group_made: bool,
    added: usize,
    /// engine version number minus the model's at the start of the trace
    v0: i64,
}

fn cred_of_payload(v: &Value) -> Option<String> {
    let sj: Value = serde_json::from_str(v["share_json"].as_str()?).ok()?;
    sj["refs"]["credential"].as_str().map(str::to_owned)
}

impl ReplayRig {
    async fn new_share(&mut self, slot: i64, v: &Value) {
        self.sid.insert(slot, v["id"].as_str().unwrap().to_owned());
        if let Some(c) = cred_of_payload(v) {
            self.cred.insert(slot, c);
        }
    }

    /// Set `expires_at` of every share the model has to the model's view of the clock.
    async fn sync_clock(&mut self, st: &Value) {
        let now = int(&st["now"]);
        for (slot, s) in map_values(&st["sh"]) {
            let (Some(id), "active" | "drifted") = (self.sid.get(&slot), text(&s["st"])) else {
                continue;
            };
            let past = now >= int(&s["exp"]);
            let sql = if past {
                format!(
                    "UPDATE _oo_docs.share SET expires_at = clock_timestamp() - interval '1 second' WHERE id = '{id}'"
                )
            } else {
                format!(
                    "UPDATE _oo_docs.share SET expires_at = clock_timestamp() + interval '1 day' WHERE id = '{id}'"
                )
            };
            self.fx
                .admin
                .execute(sqlx::AssertSqlSafe(sql))
                .await
                .unwrap();
        }
    }

    /// New shares the model created in this step (slots `first..now_n`) are the successors of `old` slots, in order.
    async fn adopt_successors(&mut self, old: &[i64], first: i64, refreshed: &[Value]) {
        for (k, o) in old.iter().enumerate() {
            let want = self.sid[o].clone();
            let new = refreshed
                .iter()
                .find(|r| r["refreshed_from"] == json!(want))
                .unwrap_or_else(|| panic!("no successor of {want} in {refreshed:?}"));
            self.new_share(first + i64::try_from(k).unwrap(), new).await;
        }
    }
}

async fn replay_share_trace(name: &str, t: &Value) -> (usize, usize) {
    let mut truncated = 0usize;
    let (e, fx) = world(&format!("fm6_rep_{}", name.replace(['.', '-'], "_"))).await;
    let mut fx = fx;
    // the engine's own publisher may add a version of its own right after start: wait until the number is stable
    let mut v0 = max_version(&mut fx).await;
    loop {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let now = max_version(&mut fx).await;
        if now == v0 {
            break;
        }
        v0 = now;
    }
    let mut rig = ReplayRig {
        v0: v0 - 1,
        e,
        fx,
        sid: BTreeMap::new(),
        cred: BTreeMap::new(),
        group_made: false,
        added: 0,
    };
    let states = t["states"].as_array().unwrap();
    let mut steps = 0usize;
    for (i, st) in states.iter().enumerate().skip(1) {
        let who = format!("{name} step {i}");
        let prev = &states[i - 1];
        let picks = &st["mbt::nondetPicks"];
        let pick =
            |k: &str| -> Option<&Value> { (picks[k]["tag"] == "Some").then(|| &picks[k]["value"]) };
        let act = text(&st["lastAct"]).to_owned();
        let nsh_prev = int(&prev["nsh"]);
        match act.as_str() {
            "mount" | "mountLate" => {
                let pol = ["manual", "at-checkpoint", "pinned"]
                    [usize::try_from(int(pick("pol0").unwrap())).unwrap()];
                let grp = int(pick("g0").unwrap()) == 0;
                let mut extra = json!({"refresh_policy": pol});
                if grp {
                    extra["group"] = json!("TICKET-FM6");
                }
                let v = mk_share(&rig.e, &["Decisions"], extra).await;
                rig.new_share(nsh_prev, &v).await;
                rig.group_made = rig.group_made || grp;
            }
            "publish" => {
                rig.added += 1;
                publish_extra(
                    &rig.e,
                    &mut rig.fx,
                    &format!("Decisions/新决定-rep-{}.md", rig.added),
                )
                .await;
            }
            "refresh" => {
                let s = int(pick("s").unwrap());
                let r = post_retry(
                    &rig.e,
                    &format!("/v1/docs/shares/{}:refresh", rig.sid[&s]),
                    &json!({}),
                )
                .await;
                assert_eq!(r.status, 200, "{who}: {}", String::from_utf8_lossy(&r.body));
                let v = r.json();
                rig.new_share(nsh_prev, &v).await;
            }
            "groupRefresh" | "groupRefreshRolledBack" => {
                let ms: Vec<i64> = map_values(&prev["sh"])
                    .into_iter()
                    .filter(|(_, s)| {
                        matches!(text(&s["st"]), "active" | "drifted")
                            && int(&s["grp"]) == 0
                            && int(&prev["now"]) < int(&s["exp"])
                    })
                    .map(|(k, _)| k)
                    .collect();
                if act == "groupRefreshRolledBack" {
                    // one member cannot compile: the whole refresh rolls back
                    let first = rig.sid[&ms[0]].clone();
                    rig.fx.admin.execute(sqlx::AssertSqlSafe(format!("UPDATE _oo_docs.share SET compile_params = jsonb_set(compile_params, '{{scope}}', '[\"Nowhere\"]') WHERE id = '{first}'"))).await.unwrap();
                    let bad = post_retry(
                        &rig.e,
                        "/v1/docs/share-groups/TICKET-FM6:refresh",
                        &json!({}),
                    )
                    .await;
                    assert!(
                        bad.status >= 400,
                        "{who}: rolled back refresh answered {}",
                        bad.status
                    );
                    rig.fx.admin.execute(sqlx::AssertSqlSafe(format!("UPDATE _oo_docs.share SET compile_params = jsonb_set(compile_params, '{{scope}}', '[\"Decisions\"]') WHERE id = '{first}'"))).await.unwrap();
                } else {
                    let r = post_retry(
                        &rig.e,
                        "/v1/docs/share-groups/TICKET-FM6:refresh",
                        &json!({}),
                    )
                    .await;
                    assert_eq!(r.status, 200, "{who}: {}", String::from_utf8_lossy(&r.body));
                    let v = r.json();
                    let refreshed = v["refreshed"].as_array().unwrap().clone();
                    rig.adopt_successors(&ms, nsh_prev, &refreshed).await;
                }
            }
            "check" => {
                let s = int(pick("s").unwrap());
                let since = int(pick("since0").unwrap());
                let unr = pick("unr").unwrap().as_bool().unwrap();
                let chk = &st["chk"];
                if !unr {
                    let c = post_retry(
                        &rig.e,
                        &format!("/v1/docs/shares/{}:check?since={since}", rig.sid[&s]),
                        &json!({}),
                    )
                    .await
                    .json();
                    assert_eq!(c["exit_code"], int(&chk["code"]), "{who}: exit code {c}");
                    if chk["refreshed"].as_bool().unwrap() {
                        // at-checkpoint: the share (or its whole group) was upgraded
                        let ms: Vec<i64> = if int(&map_values(&prev["sh"])[&s]["grp"]) == 0 {
                            map_values(&prev["sh"])
                                .into_iter()
                                .filter(|(_, m)| {
                                    matches!(text(&m["st"]), "active" | "drifted")
                                        && int(&m["grp"]) == 0
                                        && int(&prev["now"]) < int(&m["exp"])
                                })
                                .map(|(k, _)| k)
                                .collect()
                        } else {
                            vec![s]
                        };
                        let refreshed: Vec<Value> = if ms.len() > 1 || c["refreshed"].is_array() {
                            c["refreshed"].as_array().cloned().unwrap_or_default()
                        } else {
                            vec![c["refreshed"].clone()]
                        };
                        let all: Vec<Value> = if refreshed.is_empty() {
                            vec![c["refreshed"].clone()]
                        } else {
                            refreshed
                        };
                        rig.adopt_successors(&ms, nsh_prev, &all).await;
                    } else if !c["refreshed_to"].is_null() {
                        // the model refuses a refresh only when it has no free share slot (NS = 4, a bound of the
                        // model); the engine has no such limit: the trace ends here
                        assert!(
                            nsh_prev >= 4 - 1,
                            "{who}: engine refreshed, model did not, with free slots: {c}"
                        );
                        truncated += 1;
                        rig.e.finish().await;
                        return (steps, truncated);
                    }
                }
            }
            "revoke" => {
                let s = int(pick("s").unwrap());
                let r = post_retry(
                    &rig.e,
                    &format!("/v1/docs/shares/{}:revoke", rig.sid[&s]),
                    &json!({}),
                )
                .await;
                assert_eq!(r.status, 200, "{who}");
            }
            "tick" => {}
            "expireSweep" => {
                rig.sync_clock(prev).await;
                oo_docs::share::lifecycle::expire_due(&rig.e.pool)
                    .await
                    .unwrap();
            }
            "refsRead" => {
                let s = int(pick("s").unwrap());
                if let Some(c) = rig.cred.get(&s) {
                    let auth = format!("Bearer {c}");
                    let r = common::http::request(
                        rig.e.addr,
                        "GET",
                        &format!("/v1/docs/shares/{}", rig.sid[&s]),
                        &[("Authorization", &auth)],
                        None,
                    )
                    .await;
                    let ok = int(&st["chk"]["code"]) == 1;
                    assert_eq!(
                        r.status == 200,
                        ok,
                        "{who}: credential read answered {} model says valid={ok}",
                        r.status
                    );
                }
            }
            other => panic!("{who}: unexpected action {other}"),
        }
        rig.sync_clock(st).await;
        // compare every slot the model has
        for (slot, m) in map_values(&st["sh"]) {
            if text(&m["st"]) == "absent" {
                continue;
            }
            let sid = rig
                .sid
                .get(&slot)
                .unwrap_or_else(|| panic!("{who}: slot {slot} has no engine share"));
            let g = share(&rig.e, sid).await;
            let want_state = text(&m["st"]);
            // a share past its end date reads `active` until the sweep, as in the model
            assert_eq!(g["state"], want_state, "{who}: slot {slot} state {g}");
            assert_eq!(
                g["version"],
                int(&m["ver"]) + rig.v0,
                "{who}: slot {slot} version"
            );
            assert_eq!(
                g["refresh_policy"],
                ["manual", "at-checkpoint", "pinned"][usize::try_from(int(&m["pol"])).unwrap()],
                "{who}: slot {slot} policy"
            );
            let src = int(&m["src"]);
            if src >= 0 {
                assert_eq!(
                    g["refreshed_from"],
                    json!(rig.sid[&src]),
                    "{who}: slot {slot} refreshed_from"
                );
            } else {
                assert!(
                    g["refreshed_from"].is_null(),
                    "{who}: slot {slot} has refreshed_from {g}"
                );
            }
            let ups = updates(&rig.e, sid).await;
            assert_eq!(
                ups.len() as i64,
                int(&m["nupd"]),
                "{who}: slot {slot} UPDATES entries"
            );
            for (k, u) in ups.iter().enumerate() {
                assert_eq!(u["seq"], (k + 1) as i64);
                assert_eq!(u["level"], "info", "{who}: only adds were published");
            }
        }
        steps += 1;
    }
    rig.e.finish().await;
    (steps, truncated)
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn fm6_itf_traces_conform_to_the_share_engine() {
    let traces = load_traces("share_lifecycle", "OO_FM6_TRACES_DIR");
    let max: usize = std::env::var("OO_FM6_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);
    let (mut n, mut steps, mut truncated) = (0usize, 0usize, 0usize);
    for (name, t) in traces.iter().take(max) {
        let (st, tr) = replay_share_trace(name, t).await;
        steps += st;
        truncated += tr;
        n += 1;
    }
    eprintln!(
        "FM6 conformance: {n} traces, {steps} steps; share state, version, policy, refreshed_from, UPDATES count, check codes and credential validity equal at every step; {truncated} trace(s) ended where the model had no free share slot"
    );
}
