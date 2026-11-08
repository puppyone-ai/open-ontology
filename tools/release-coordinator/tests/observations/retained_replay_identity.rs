//! Replay of an already-open fence re-mints a missing or invalid logical
//! publication identity (0135/0136) through the publisher's fence write.
//! Owned scratch PostgreSQL 17 only:
//! `OO_RETAINED_IDENTITY_PG='host=... port=... user=postgres dbname=...'`.
use super::*;

const FENCE: &str = include_str!("../../migrations/oo_publish_target_v1.sql");
const IDENTITY: &str =
    include_str!("../../../../crates/oo-pipeline/migrations/0135_logical_publication_identity.sql");
const ROUTINES: &str =
    include_str!("../../../../crates/oo-pipeline/migrations/0136_publication_routine_identity.sql");

async fn replay(client: &mut tokio_postgres::Client, relation: &str) -> Option<&'static str> {
    let tx = client.transaction().await.unwrap();
    // The publisher's own privileges, never the administrator's.
    tx.batch_execute("SET LOCAL ROLE oo_target_admin")
        .await
        .unwrap();
    let outcome = replay_identity(&tx, "oo_r_replay", relation).await.unwrap();
    let expected = (outcome != Some("not_installed")).then_some(outcome.is_some());
    assert_eq!(
        identity_current(&tx, "oo_r_replay", relation)
            .await
            .unwrap(),
        expected
    );
    tx.commit().await.unwrap();
    outcome
}

#[tokio::test]
#[ignore = "owned scratch PostgreSQL 17; set OO_RETAINED_IDENTITY_PG"]
async fn replay_remints_missing_or_invalid_identity() {
    let conninfo = std::env::var("OO_RETAINED_IDENTITY_PG").expect("owned scratch PG");
    let (mut client, connection) = tokio_postgres::connect(&conninfo, tokio_postgres::NoTls)
        .await
        .unwrap();
    let driver = tokio::spawn(connection);
    client
        .batch_execute(&format!(
            "CREATE SCHEMA _oo; CREATE ROLE oo_target_admin NOLOGIN; CREATE ROLE oo_r_replay NOLOGIN;
             {FENCE}
             GRANT USAGE ON SCHEMA _oo,oo_publish_target TO oo_target_admin;
             GRANT SELECT,UPDATE ON oo_publish_target.fence TO oo_target_admin;
             CREATE TABLE _oo.replay_base(amount bigint NOT NULL);
             CREATE FUNCTION _oo.replay_scale(x bigint) RETURNS bigint LANGUAGE sql IMMUTABLE RETURN x;
             CREATE VIEW _oo.replay_view AS SELECT _oo.replay_scale(amount) AS amount FROM _oo.replay_base;
             GRANT SELECT,TRIGGER ON _oo.replay_base TO oo_target_admin;
             GRANT SELECT ON _oo.replay_view TO oo_target_admin;
             INSERT INTO oo_publish_target.fence VALUES
                 (true,'open','oo_r_replay','_oo','replay_base','{{amount}}',repeat('a',64),'g1',NULL,NULL,0),
                 (true,'open','oo_r_replay','_oo','replay_view','{{amount}}',repeat('a',64),'g1',NULL,NULL,0);"
        ))
        .await
        .unwrap();
    // Fences opened before the identity migration: nothing to check yet.
    assert_eq!(
        replay(&mut client, "replay_base").await,
        Some("not_installed")
    );
    client.batch_execute(IDENTITY).await.unwrap();
    client.batch_execute(ROUTINES).await.unwrap();
    // Installing the migrations mints nothing: the open fences have no record.
    for relation in ["replay_base", "replay_view"] {
        assert_eq!(replay(&mut client, relation).await, Some("reminted"));
        assert_eq!(replay(&mut client, relation).await, Some("current"));
    }
    // Tombstoned by DDL on a routine the view depends on: re-minted on replay.
    client
        .batch_execute(
            "CREATE OR REPLACE FUNCTION _oo.replay_scale(x bigint) RETURNS bigint LANGUAGE sql IMMUTABLE RETURN x+1",
        )
        .await
        .unwrap();
    let valid: bool = client
        .query_one(
            "SELECT valid FROM _oo.publication_identity WHERE relation_name='replay_view'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(!valid);
    assert_eq!(replay(&mut client, "replay_base").await, Some("current"));
    assert_eq!(replay(&mut client, "replay_view").await, Some("reminted"));
    // Never weaker than the Gateway predicate: a record for another Pack is
    // not current, and the publisher cannot mint one without TRIGGER/SELECT.
    client
        .batch_execute(
            "UPDATE _oo.publication_identity SET publication_id=repeat('b',64) WHERE relation_name='replay_base';
             REVOKE SELECT,TRIGGER ON _oo.replay_base FROM oo_target_admin",
        )
        .await
        .unwrap();
    let tx = client.transaction().await.unwrap();
    tx.batch_execute("SET LOCAL ROLE oo_target_admin")
        .await
        .unwrap();
    assert_eq!(
        identity_current(&tx, "oo_r_replay", "replay_base")
            .await
            .unwrap(),
        Some(false)
    );
    assert!(
        replay_identity(&tx, "oo_r_replay", "replay_base")
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    drop(client);
    driver.await.unwrap().unwrap();
}
