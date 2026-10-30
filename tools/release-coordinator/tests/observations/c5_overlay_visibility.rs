use super::*;
use tokio_postgres::{Client, NoTls};

async fn connect(user: &str) -> Client {
    let socket = std::env::var("OO_C5_VISIBILITY_SOCKET").expect("owned fixture runner required");
    assert!(socket.contains("/oo-c5-visibility-"));
    let port = std::env::var("OO_C5_VISIBILITY_PORT").unwrap().parse::<u16>().unwrap();
    let (client, connection) = tokio_postgres::Config::new()
        .host(&socket).port(port).dbname("visibility_fixture").user(user)
        .connect(NoTls).await.unwrap();
    tokio::spawn(async move { let _ = connection.await; });
    client
}

async fn reader() -> Client {
    let client = connect("oo_visibility_login").await;
    client.batch_execute("SET ROLE oo_r_visibility").await.unwrap();
    client
}

async fn count(client: &Client, relation: &str) -> i64 {
    client.query_one(&format!("SELECT count(*) FROM _oo.{}", quote_ident(relation).unwrap()), &[])
        .await.unwrap().get(0)
}

#[tokio::test]
#[ignore = "run tests/c5_overlay_visibility_fixture.py; new owned PG17 socket cluster only"]
async fn c5_overlay_visibility_lifecycle() {
    let mut admin = connect("oo_target_admin").await;
    let root = connect("postgres").await;
    let context = |raw: String| serde_json::from_str::<Value>(&raw).unwrap();
    let view = context(admin.query_one("SELECT ctx::text FROM overlay_fixture.saved WHERE label='gated'", &[]).await.unwrap().get(0));
    let base = context(admin.query_one("SELECT ctx::text FROM overlay_fixture.saved WHERE label='base'", &[]).await.unwrap().get(0));
    let legacy = context(admin.query_one("SELECT ctx::text FROM overlay_fixture.saved WHERE label='legacy'", &[]).await.unwrap().get(0));
    let physical = view["physical"].as_str().unwrap();
    let columns: Vec<String> = view["pcs"].as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_owned()).collect();
    let fence = FenceRef { schema_name: "_oo", relation_name: physical, role_name: "oo_r_visibility" };
    let base_fence = FenceRef { schema_name: "_oo", relation_name: base["physical"].as_str().unwrap(), role_name: fence.role_name };
    let pack = "a".repeat(64);
    macro_rules! action {
        ($body:ident, $fence:expr $(, $arg:expr)*) => {{
            let tx = admin.transaction().await.unwrap();
            let receipt = $body(&tx, $fence $(, $arg)*).await.unwrap();
            assert_eq!(receipt.code, 0, "{:?}", receipt.body);
            tx.commit().await.unwrap();
            receipt
        }};
    }
    for name in [legacy["physical"].as_str().unwrap(), "pt_ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"] {
        let tx = admin.transaction().await.unwrap();
        let fake = FenceRef { relation_name: name, ..fence };
        assert!(install_body(&tx, &fake).await.is_err(), "ungated/unregistered view admitted");
        tx.rollback().await.unwrap();
    }
    action!(install_body, &base_fence);
    action!(close_body, &base_fence);
    let base_columns = vec![base["pcs"][0].as_str().unwrap().to_owned()];
    action!(grant_body, &base_fence, &base_columns);
    let base_reader = reader().await;
    assert_eq!(count(&base_reader, base_fence.relation_name).await, 0);
    action!(open_body, &base_fence, &base_columns, &pack, "base1", "base_candidate");
    assert_eq!(count(&base_reader, base_fence.relation_name).await, 4);
    action!(close_body, &base_fence);
    let owner = connect("oo_target_admin").await;
    owner.batch_execute("SET ROLE oo_target_owner").await.unwrap();
    assert_eq!(count(&owner, base_fence.relation_name).await, 4);
    root.batch_execute("BEGIN; INSERT INTO overlay_fixture.saved SELECT 'successor', \
        overlay_fixture.load_range('visibility','orders','1','[[\"3\",\"new3\"],[\"4\",\"new4\"]]'); COMMIT;").await.unwrap();
    let successor: String = root.query_one("SELECT ctx->>'physical' FROM overlay_fixture.saved WHERE label='successor'", &[]).await.unwrap().get(0);
    assert_eq!(count(&owner, &successor).await, 3, "Loader can build/read a new overlay after root publication closed");
    assert!(!root.query_one("SELECT rolbypassrls FROM pg_roles WHERE rolname='oo_target_owner'", &[]).await.unwrap().get::<_, bool>(0));
    let hash: String = root.query_one("SELECT view_sha256 FROM _oo.c5_overlay WHERE physical_name=$1", p!(physical)).await.unwrap().get(0);
    let digest = "a".repeat(64);
    let context = ValidatedNativeContext {
        mappings: Default::default(),
        default_mapping: Some(("visibility", &digest, view["generation"].as_i64().unwrap())),
        publication: None,
        pending_runs: [(view["run"].as_str().unwrap().to_owned(), view["fence"].as_i64().unwrap())].into(),
    };
    let binding = ("app".to_owned(), "orders".to_owned(), fence.role_name.to_owned(),
        vec!["k1".to_owned(), "value".to_owned()]);
    let tx = admin.transaction().await.unwrap();
    assert_eq!(resolve_native_binding_body(&tx, &context, &binding).await.unwrap(),
        ("_oo".to_owned(), physical.to_owned(), fence.role_name.to_owned(), columns.clone()));
    tx.commit().await.unwrap();
    action!(install_body, &fence);
    action!(close_body, &fence);
    action!(grant_body, &fence, &columns[..1]);
    let r = reader().await;
    assert_eq!(count(&r, physical).await, 0);
    let tx = admin.transaction().await.unwrap();
    assert_eq!(policy_expr(&tx, &fence).await.unwrap().as_deref(), Some("false"));
    let catalog = catalog_body(&tx, &fence).await.unwrap();
    let proof = reader_observe(&r, &fence, &columns[..1], &catalog, Some("false")).await.unwrap();
    assert_eq!(proof.body["row_count"], 0);
    assert_eq!(proof.body["hidden_denied"].as_array().unwrap().len(), 2);
    tx.commit().await.unwrap();
    action!(revoke_body, &fence);
    assert!(r.query_one(&format!("SELECT {} FROM {}", quote_ident(&columns[0]).unwrap(), table(&fence).unwrap()), &[]).await.is_err());
    action!(grant_body, &fence, &columns);
    let receipt = action!(open_body, &fence, &columns, &pack, "v1", "candidate1");
    assert_eq!(receipt.body["policy"], "true");
    assert_eq!(count(&r, physical).await, 3);
    assert_eq!(count(&owner, physical).await, 3, "closed FORCE RLS root must remain usable internally");
    let invoker: String = r.query_one("SELECT current_user::text", &[]).await.unwrap().get(0);
    assert_eq!(invoker, fence.role_name);
    assert!(!r.query_one("SELECT _oo.c5_overlay_visible($1,'oo_visibility_login')", p!(physical)).await.unwrap().get::<_, bool>(0));
    assert!(r.query_one(&format!("SELECT _oo_pos FROM {}", table(&fence).unwrap()), &[]).await.is_err());
    assert!(r.query_one(&format!("SELECT * FROM {}", table(&base_fence).unwrap()), &[]).await.is_err());
    assert!(r.query_one("SELECT * FROM _oo.c5_overlay", &[]).await.is_err());
    assert!(r.query_one("SELECT * FROM oo_publish_target.fence", &[]).await.is_err());
    assert!(r.query_one("SELECT _oo.c5_overlay_finish_before_visibility('',NULL,0,NULL)", &[]).await.is_err());
    assert_eq!(root.query_one("SELECT count(*) FROM pg_policy WHERE polrelid=to_regclass($1)", p!(&format!("_oo.{physical}"))).await.unwrap().get::<_, i64>(0), 0);
    let gate = format!("_oo.{}()", quote_ident(&format!("pv_{}", &physical[3..])).unwrap());
    assert!(!r.query_one(&format!("SELECT {gate}"), &[]).await.unwrap().get::<_, bool>(0));
    root.batch_execute(&format!("REVOKE ALL ON FUNCTION {gate} FROM oo_r_visibility")).await.unwrap();
    let tx = admin.transaction().await.unwrap();
    let status = status_body(&tx, &fence, Some(&pack), Some(&columns)).await.unwrap();
    assert_eq!(status.body["policy"], "false");
    assert_eq!(status.body["knowledge_published"], false);
    assert_ne!(open_body(&tx, &fence, &columns, &pack, "v1", "candidate1").await.unwrap().code, 0);
    tx.rollback().await.unwrap();
    root.batch_execute(&format!("GRANT EXECUTE ON FUNCTION {gate} TO oo_r_visibility")).await.unwrap();
    admin.batch_execute(include_str!("../../migrations/oo_publish_visibility_v2.sql")).await.unwrap();
    admin.batch_execute("GRANT USAGE ON SCHEMA oo_publish_target TO oo_visibility_login; \
        GRANT EXECUTE ON FUNCTION oo_publish_target.visible_vector() TO oo_visibility_login").await.unwrap();
    let mut snapshot_reader = connect("oo_visibility_login").await;
    let snapshot = snapshot_reader.build_transaction().read_only(true)
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead).start().await.unwrap();
    assert_eq!(snapshot.query_one("SELECT opened_columns FROM oo_publish_target.visible_vector() WHERE relation_name=$1",
        p!(physical)).await.unwrap().get::<_, Vec<String>>(0), columns);
    assert_eq!(policy_expr(&snapshot, &fence).await.unwrap().as_deref(), Some("true"));
    snapshot.batch_execute("SET LOCAL ROLE oo_r_visibility").await.unwrap();
    assert_eq!(snapshot.query_one(&format!("SELECT count(*) FROM {}", table(&fence).unwrap()), &[]).await.unwrap().get::<_, i64>(0), 3);
    snapshot.commit().await.unwrap();

    // A prepared reader cannot be killed: close must refuse, not claim success.
    r.batch_execute(&format!("BEGIN; SELECT {} FROM {}; PREPARE TRANSACTION 'visibility_reader'",
        quote_ident(&columns[0]).unwrap(), table(&fence).unwrap())).await.unwrap();
    let tx = admin.transaction().await.unwrap();
    assert!(close_body(&tx, &fence).await.is_err());
    tx.rollback().await.unwrap();
    root.batch_execute("ROLLBACK PREPARED 'visibility_reader'").await.unwrap();
    assert_eq!(count(&r, physical).await, 3);

    // Closing kills the login behind SET ROLE, including an already-held snapshot.
    r.batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY").await.unwrap();
    assert_eq!(count(&r, physical).await, 3);
    let closed = action!(close_body, &fence);
    assert!(closed.body["streams_stopped"].as_i64().unwrap() >= 1);
    assert!(r.simple_query("SELECT 1").await.is_err());
    action!(grant_body, &fence, &columns[..1]);
    let fresh = reader().await;
    assert_eq!(count(&fresh, physical).await, 0);
    action!(open_body, &fence, &columns[..1], &pack, "v2", "candidate2");
    assert_eq!(count(&fresh, physical).await, 3);
    assert!(fresh.query_one(&format!("SELECT {} FROM {}", quote_ident(&columns[1]).unwrap(), table(&fence).unwrap()), &[]).await.is_err());
    action!(close_body, &fence);
    let reopened = action!(open_body, &fence, &columns, &pack, "v3", "candidate3");
    assert_eq!(reopened.body["revoked_columns"], json!([columns[1]]));
    let r = reader().await;
    assert_eq!(count(&r, physical).await, 3);
    assert!(r.query_one(&format!("SELECT {} FROM {}", quote_ident(&columns[1]).unwrap(), table(&fence).unwrap()), &[]).await.is_err());
    assert_eq!(hash, root.query_one("SELECT view_sha256 FROM _oo.c5_overlay WHERE physical_name=$1", p!(physical)).await.unwrap().get::<_, String>(0));

    // New login after termination, but before the close commit: its old MVCC
    // fence row still says open. The current ACL latch must nevertheless deny.
    let tx = admin.transaction().await.unwrap();
    close_body(&tx, &fence).await.unwrap();
    let late = reader().await;
    late.batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY; SELECT pg_current_snapshot()").await.unwrap();
    tx.commit().await.unwrap();
    action!(grant_body, &fence, &columns[..1]);
    assert_eq!(count(&late, physical).await, 0, "snapshot born during close bypassed staged fence");
    late.batch_execute("ROLLBACK").await.unwrap();
    action!(open_body, &fence, &columns[..1], &pack, "v4", "candidate4");

    // Definition and reloptions tampering invalidate the registry, even with the same OID.
    root.batch_execute(&format!("ALTER VIEW {} SET (security_barrier=false)", table(&fence).unwrap())).await.unwrap();
    let tx = admin.transaction().await.unwrap();
    assert!(policy_expr(&tx, &fence).await.is_err());
    tx.rollback().await.unwrap();
    root.batch_execute(&format!("ALTER VIEW {} SET (security_barrier=true)", table(&fence).unwrap())).await.unwrap();
    let tx = admin.transaction().await.unwrap();
    assert_eq!(policy_expr(&tx, &fence).await.unwrap().as_deref(), Some("true"));
    tx.commit().await.unwrap();
    let delta: String = root.query_one("SELECT delta_name FROM _oo.c5_overlay WHERE physical_name=$1", p!(physical)).await.unwrap().get(0);
    let tx = admin.transaction().await.unwrap();
    tx.batch_execute(&format!("CREATE OR REPLACE VIEW {} WITH (security_barrier=true) AS \
        SELECT * FROM _oo.{} WHERE _oo.c5_overlay_visible('{physical}',CURRENT_USER) OR true \
        UNION ALL SELECT * FROM _oo.{} WHERE false",
        table(&fence).unwrap(), quote_ident(&delta).unwrap(), quote_ident(&delta).unwrap())).await.unwrap();
    assert!(resolve_native_binding_body(&tx, &context, &binding).await.is_err());
    assert!(install_body(&tx, &fence).await.is_err());
    tx.rollback().await.unwrap();
    // A failed open is atomic; neither the row gate nor column grants survive rollback.
    action!(close_body, &fence);
    let tx = admin.transaction().await.unwrap();
    open_body(&tx, &fence, &columns[..1], &pack, "rolled_back", "rollback_candidate").await.unwrap();
    tx.rollback().await.unwrap();
    let tx = admin.transaction().await.unwrap();
    assert_eq!(policy_expr(&tx, &fence).await.unwrap().as_deref(), Some("false"));
    tx.commit().await.unwrap();
    let r = reader().await;
    assert!(r.query_one(&format!("SELECT {} FROM {}", quote_ident(&columns[0]).unwrap(), table(&fence).unwrap()), &[]).await.is_err());
    action!(close_body, &fence);
    action!(open_body, &fence, &columns[..1], &pack, "v5", "candidate5");
    // Synthetic stored library receipt: exercise the real failure callback,
    // not a claim of end-to-end library publication acceptance.
    root.execute("INSERT INTO _oo.native_library_publication \
        (receipt_sha256,library_id,library,shards,publication_xid) VALUES \
        (repeat('c',64),'visibility_library','{}',jsonb_build_array(jsonb_build_object('receipt', \
        jsonb_build_object('tables',jsonb_build_array(jsonb_build_object('physical_name',$1::text), \
        jsonb_build_object('physical_name',$2::text))))),pg_current_xact_id())",
        p!(physical, base_fence.relation_name)).await.unwrap();
    root.batch_execute("INSERT INTO _oo.native_library_head VALUES \
        ('visibility_library',repeat('d',64),repeat('c',64),'ready'); \
        GRANT UPDATE ON oo_publish_target.fence TO oo_target_owner").await.unwrap();
    admin.batch_execute("SELECT _oo.native_library_fail_closed('visibility_library',repeat('c',64))").await.unwrap();
    assert_eq!(root.query_one("SELECT state FROM _oo.native_library_head WHERE library_id='visibility_library'", &[]).await.unwrap().get::<_, String>(0), "closed");
    let tx = admin.transaction().await.unwrap();
    assert_eq!(policy_expr(&tx, &fence).await.unwrap().as_deref(), Some("false"));
    assert_eq!(lock_fence(&tx, &fence).await.unwrap().opened, columns[..1]);
    tx.commit().await.unwrap();
    assert_eq!(count(&owner, physical).await, 3);
    let r = reader().await;
    assert!(r.query_one(&format!("SELECT {} FROM {}", quote_ident(&columns[0]).unwrap(), table(&fence).unwrap()), &[]).await.is_err());
    action!(grant_body, &fence, &columns[..1]);
    let tx = admin.transaction().await.unwrap();
    open_body(&tx, &fence, &columns[..1], &pack, "prepared", "prepared_candidate").await.unwrap();
    tx.batch_execute("PREPARE TRANSACTION 'visibility_open'").await.unwrap();
    // PREPARE ended the server transaction; dropping the wrapper cannot roll it back.
    drop(tx);
    r.batch_execute("SET lock_timeout='250ms'").await.unwrap();
    let blocked = r.query_one(&format!("SELECT count(*) FROM {}", table(&fence).unwrap()), &[]).await.unwrap_err();
    assert_eq!(blocked.code().map(|c| c.code()), Some("55P03"), "prepared publication must exclude readers");
    root.batch_execute("COMMIT PREPARED 'visibility_open'").await.unwrap();
    r.batch_execute("RESET lock_timeout").await.unwrap();
    assert_eq!(count(&r, physical).await, 3, "prepared commit opens ACL and row gate together");
    action!(close_body, &fence);
    println!("PASS: genuine view gate; staged zero; open rows; column ACL/revoke history; closed root FORCE RLS; prepared reader refusal and atomic prepared publication; SET ROLE stale-reader termination; close-racing snapshot denied; native mapping; reader snapshot; atomic rollback; library fail-closed callback; tampering/legacy/forged rejection");
}
