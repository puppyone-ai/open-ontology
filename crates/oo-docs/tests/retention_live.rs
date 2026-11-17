//! Version retention on a live PostgreSQL (B-S6-12, D58): the caches of a
//! version that nothing pins and that is 30 days old go; a pinned version, a
//! young one, the newest one and objects another pack still uses stay, and the
//! same compile writes the same bytes back.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test retention_live -- --ignored --test-threads=1`

mod common;

use common::CLUSTER_LOCK;
use common::fx::{DRIVE, Fx, uuid};
use oo_docs::cas::CasKind;
use oo_docs::compile::{CompileRequest, persist};
use oo_docs::publish::run::PublishOutcome;
use oo_docs::store::retention::{RETAIN_DAYS, sweep};

async fn pack(fx: &mut Fx, task: &str, version: Option<i64>) -> (String, i64, Vec<String>) {
    let req = CompileRequest {
        version,
        scope: vec![],
        task: task.to_owned(),
        budget: 800,
        ..CompileRequest::default()
    };
    let norm = oo_docs::compile::normalize_scope(&req.scope);
    let reader =
        oo_docs::compile::pg::PgReader::load(&mut fx.rt, &fx.cas, DRIVE, req.version, &norm)
            .await
            .unwrap();
    let p = oo_docs::compile::compile(&reader, &req).unwrap();
    let (rec, _) = persist::persist(&mut fx.rt, &fx.cas, &p).await.unwrap();
    let b3s = fx
        .text(&format!(
            "SELECT cas_b3 FROM _oo_docs.context_pack_file WHERE pack_id = '{}' ORDER BY path",
            rec.id
        ))
        .await;
    (rec.id, p.version.n, b3s)
}

fn doc(owner: &str) -> String {
    format!("---\ntype: project\nname: 星火\nowner: {owner}\n---\n\n# 星火\n\n负责人：{owner}\n")
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn only_old_unpinned_versions_lose_their_cached_pack_bytes() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_retention";
    let mut fx = Fx::new(db).await;
    fx.index_md("Projects/星火.md", &doc("陈子墨")).await;
    let PublishOutcome::Published(v1) = fx.publish().await else {
        panic!("v1")
    };
    let (pid1, n1, bytes1) = pack(&mut fx, "第一版", None).await;
    assert_eq!(n1, v1.n);
    fx.index_md(
        "People/周若溪.md",
        "---\ntype: person\nname: 周若溪\nrole: 产品\n---\n\n# 周若溪\n\n角色：产品\n",
    )
    .await;
    let PublishOutcome::Published(v2) = fx.publish().await else {
        panic!("v2")
    };
    let (_, n2, bytes2) = pack(&mut fx, "第二版", None).await;
    assert_eq!(n2, v2.n);
    assert!(!bytes1.is_empty() && !bytes2.is_empty());

    // Nothing is old yet: nothing goes.
    assert_eq!(
        sweep(&mut fx.rt, &fx.cas, RETAIN_DAYS)
            .await
            .unwrap()
            .objects,
        0
    );

    // Age version 1 by 40 days (the table is append-only: the guard trigger is
    // off for this one statement, in the admin session).
    fx.admin_sql("ALTER TABLE _oo_docs.ontology_version DISABLE TRIGGER USER")
        .await;
    fx.admin_sql(&format!(
        "UPDATE _oo_docs.ontology_version SET published_at = clock_timestamp() - interval '40 days' WHERE n = {n1}"
    ))
    .await;
    fx.admin_sql("ALTER TABLE _oo_docs.ontology_version ENABLE TRIGGER USER")
        .await;

    // A live share pins version 1: it stays.
    let pid = pid1;
    let share = uuid();
    fx.admin_sql(&format!(
        "INSERT INTO _oo_docs.share (id, pack_id, drive_id, scope, role, budget, status, grantor, mount_ref, expires_at) \
         VALUES ('{share}', '{pid}', '{DRIVE}', '{{}}', 'viewer', 1000, 'active', 'owner', 'm', now() + interval '1 day')"
    ))
    .await;
    assert_eq!(
        fx.scalar(&format!(
            "SELECT pinned_by FROM _oo_docs.ontology_version_pins WHERE n = {n1}"
        ))
        .await,
        1
    );
    assert_eq!(
        sweep(&mut fx.rt, &fx.cas, RETAIN_DAYS)
            .await
            .unwrap()
            .objects,
        0,
        "pinned"
    );

    // Revoked: nothing pins it, so its bytes go, and only its own.
    fx.admin_sql(&format!(
        "UPDATE _oo_docs.share SET status = 'revoked', revoked_at = now() WHERE id = '{share}'"
    ))
    .await;
    let only1: Vec<&String> = bytes1.iter().filter(|b| !bytes2.contains(b)).collect();
    let r = sweep(&mut fx.rt, &fx.cas, RETAIN_DAYS).await.unwrap();
    assert_eq!(r.versions, [n1]);
    assert_eq!(r.objects as usize, only1.len(), "{r:?}");
    for b in &only1 {
        assert!(fx.cas.get(CasKind::Pack, b).is_err(), "swept");
    }
    for b in &bytes2 {
        assert!(
            fx.cas.get(CasKind::Pack, b).is_ok(),
            "the newest version is never swept"
        );
    }
    // The rows stay, and the same compile writes the same bytes back.
    assert_eq!(
        fx.scalar(&format!(
            "SELECT count(*) FROM _oo_docs.context_pack WHERE version_n = {n1}"
        ))
        .await,
        1
    );
    // Compiling the same request again heals them: put the same bytes back.
    let before = fx
        .scalar("SELECT count(*) FROM _oo_docs.context_pack")
        .await;
    let _ = pack(&mut fx, "第二版", None).await;
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.context_pack")
            .await,
        before,
        "cache hit, no new row"
    );
    fx.finish(db).await;
}
