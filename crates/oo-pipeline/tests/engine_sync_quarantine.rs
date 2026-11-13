//! Owned target test; never connect using ambient deployment credentials.
use oo_connectors::engine::{
    ChangeBatch, ChunkReceipt, ChunkSpec, ChunkStrategy, ENGINE_PROTOCOL, KeyShape, KeyValue,
    RowChange, SourceColumnShape, TableChunkPlan, TableRef, Watermark,
};
use oo_pipeline::{
    engine_load::Target,
    engine_load_pg::PgTarget,
    engine_sync_pg::{AlignedRebaseline, PgSync},
    native_arrow::{DecodedField, DecodedStream, NativeCell},
};
use sqlx::{Connection, PgConnection, postgres::PgConnectOptions};

fn image(id: i64, value: &str) -> DecodedStream {
    DecodedStream {
        rows: 1,
        fields: vec![
            DecodedField {
                name: "id".into(),
                nullable: false,
                arrow_type: "int64".into(),
                precision: Some(64),
                scale: None,
                timezone: None,
            },
            DecodedField {
                name: "payload".into(),
                nullable: false,
                arrow_type: "utf8".into(),
                precision: None,
                scale: None,
                timezone: None,
            },
        ],
        columns: vec![
            vec![NativeCell::Int {
                bit_width: 64,
                value: id,
            }],
            vec![NativeCell::Utf8(value.into())],
        ],
    }
}

fn plan(table: &TableRef) -> TableChunkPlan {
    TableChunkPlan {
        protocol: ENGINE_PROTOCOL.into(),
        table: table.clone(),
        key: KeyShape::IntegerPrimaryKey {
            column: "id".into(),
        },
        strategy: ChunkStrategy::Keyset {
            columns: vec!["id".into()],
        },
        chunks: vec![ChunkSpec {
            table: table.clone(),
            plan_revision: 1,
            chunk_id: 0,
            lo: None,
            hi: None,
            estimated_rows: Some(1),
        }],
        estimated_rows: Some(1),
    }
}

async fn baseline(conn: &mut PgConnection, source: &str, table: &TableRef) -> String {
    let plan = plan(table);
    let mut target = PgTarget::new(conn, source, "base", table).await.unwrap();
    target.begin(&plan).await.unwrap();
    target
        .stage(
            &ChunkReceipt {
                table: table.clone(),
                plan_revision: 1,
                chunk_id: 0,
                rows: 1,
                bytes: 1,
                sha256: "a".repeat(64),
                low: None,
                high: None,
            },
            &image(1, "base"),
        )
        .await
        .unwrap();
    target.publish(&plan).await.unwrap();
    target.relation()
}

#[tokio::test]
#[ignore = "requires a disposable owned PG target and OO_QUARANTINE_PASSWORD_FILE/PORT"]
async fn quarantined_table_freezes_while_healthy_table_commits_atomically() {
    assert_eq!(std::env::var("OO_QUARANTINE_OWNED").as_deref(), Ok("1"));
    let password = std::fs::read_to_string(
        std::env::var("OO_QUARANTINE_PASSWORD_FILE").expect("password file"),
    )
    .unwrap();
    let options = PgConnectOptions::new()
        .host("127.0.0.1")
        .port(
            std::env::var("OO_QUARANTINE_PORT")
                .unwrap()
                .parse()
                .unwrap(),
        )
        .username("postgres")
        .database("postgres")
        .password(password.trim());
    let mut conn = PgConnection::connect_with(&options).await.unwrap();
    let positions = [
        (
            "pg",
            Watermark::PgLsn { lsn: 10 },
            Watermark::PgLsn { lsn: 20 },
            Watermark::PgLsn { lsn: 30 },
        ),
        (
            "mysql",
            Watermark::MysqlFilePos {
                file: "bin.000009".into(),
                position: 100,
            },
            Watermark::MysqlFilePos {
                file: "bin.000010".into(),
                position: 4,
            },
            Watermark::MysqlFilePos {
                file: "bin.000010".into(),
                position: 200,
            },
        ),
        (
            "oracle",
            Watermark::OracleScn { scn: 10 },
            Watermark::OracleScn { scn: 20 },
            Watermark::OracleScn { scn: 30 },
        ),
    ];
    for (source, initial, next, last) in positions {
        let tables = ["bad", "good"].map(|name| TableRef {
            schema: "fixture".into(),
            name: format!("{source}_{name}"),
        });
        let bad_view = baseline(&mut conn, source, &tables[0]).await;
        let good_view = baseline(&mut conn, source, &tables[1]).await;
        let mut target = PgSync::open(&mut conn, source, "epoch", &tables, &initial)
            .await
            .unwrap();
        assert_eq!(
            target.quarantine(&tables[0], "untrusted detail").await,
            Err("sync_table_reason")
        );
        target
            .quarantine(&tables[0], "source_permission")
            .await
            .unwrap();
        let frozen = target.table_positions().await.unwrap()[0].clone();
        assert_eq!(frozen.position, initial);
        let batch = ChangeBatch {
            position: next.clone(),
            changes: tables
                .iter()
                .map(|table| RowChange::Update {
                    table: table.clone(),
                    key: vec![KeyValue::Int(1)],
                    old_key: None,
                })
                .collect(),
            images: vec![],
        };
        // Even an undecodable stale image cannot enter the retained table.
        let invalid = DecodedStream {
            rows: 0,
            fields: vec![],
            columns: vec![],
        };
        assert_eq!(
            target
                .apply(&initial, &batch, &[invalid.clone(), invalid.clone()])
                .await,
            Err("sync_image_shape")
        );
        assert_eq!(target.position().await.unwrap(), initial);
        assert_eq!(target.metrics().await.unwrap(), (0, 0));
        target
            .apply(&initial, &batch, &[invalid, image(1, "healthy")])
            .await
            .unwrap();
        assert_eq!(target.position().await.unwrap(), next);
        assert_eq!(target.feedback_position().await.unwrap(), initial);
        let status = target.table_positions().await.unwrap();
        assert_eq!(status[0].position, initial);
        assert_eq!(status[0].stale_since, frozen.stale_since);
        assert_eq!(status[1].position, next);
        assert!(status[1].stale_reason.is_none());
        drop(target);
        let mut target = PgSync::open(&mut conn, source, "epoch", &tables, &initial)
            .await
            .unwrap();
        assert_eq!(target.table_positions().await.unwrap()[0].position, initial);
        target
            .quarantine(&tables[0], "schema_drift:c.payload:renamed")
            .await
            .unwrap();
        assert_eq!(
            target.table_positions().await.unwrap()[0].stale_since,
            frozen.stale_since
        );
        target
            .quarantine(&tables[1], "source_timeout")
            .await
            .unwrap();
        assert_eq!(
            target
                .apply(
                    &next,
                    &ChangeBatch {
                        position: last.clone(),
                        changes: vec![],
                        images: vec![],
                    },
                    &[],
                )
                .await,
            Err("sync_all_stale")
        );
        assert_eq!(target.position().await.unwrap(), next);
        let plan = plan(&tables[0]);
        let receipt = ChunkReceipt {
            table: tables[0].clone(),
            plan_revision: 1,
            chunk_id: 0,
            rows: 1,
            bytes: 1,
            sha256: "b".repeat(64),
            low: Some(next.clone()),
            high: Some(last.clone()),
        };
        let columns = [
            SourceColumnShape {
                name: "id".into(),
                ordinal: 1,
                source_type: "bigint".into(),
                target_type: "bigint".into(),
                nullable: false,
                charset: None,
                collation: None,
                primary_key_ordinal: Some(1),
                unique_key_ordinals: Default::default(),
            },
            SourceColumnShape {
                name: "payload".into(),
                ordinal: 2,
                source_type: "text".into(),
                target_type: "text".into(),
                nullable: false,
                charset: None,
                collation: None,
                primary_key_ordinal: None,
                unique_key_ordinals: Default::default(),
            },
        ];
        let projection = ["id".into(), "payload".into()];
        let mut wrong = columns.clone();
        wrong[0].target_type = "integer".into();
        assert_eq!(
            target
                .rejoin(AlignedRebaseline {
                    run: "aligned-repair",
                    plan: &plan,
                    receipt: &receipt,
                    rows: &image(1, "must-rollback"),
                    columns: &wrong,
                    projection: &projection,
                })
                .await,
            Err("load_column_catalog_mismatch")
        );
        assert_eq!(target.table_positions().await.unwrap()[0].position, initial);
        assert!(
            target.table_positions().await.unwrap()[0]
                .stale_reason
                .is_some()
        );
        target
            .rejoin(AlignedRebaseline {
                run: "aligned-repair",
                plan: &plan,
                receipt: &receipt,
                rows: &image(1, "rebased"),
                columns: &columns,
                projection: &projection,
            })
            .await
            .unwrap();
        let repaired = &target.table_positions().await.unwrap()[0];
        assert_eq!(repaired.position, last);
        assert!(repaired.stale_reason.is_none());
        assert!(repaired.stale_since.is_none());
        // The healthy source cursor still trails the per-table repair. Its
        // replay must not resurrect a deleted key or duplicate a repaired row.
        target
            .apply(
                &next,
                &ChangeBatch {
                    position: last.clone(),
                    changes: vec![RowChange::Insert {
                        table: tables[0].clone(),
                        key: vec![KeyValue::Int(2)],
                    }],
                    images: vec![],
                },
                &[image(2, "obsolete")],
            )
            .await
            .unwrap();
        assert_eq!(target.position().await.unwrap(), last);
        assert_eq!(target.feedback_position().await.unwrap(), next);
        drop(target);
        let mut target = PgSync::open(&mut conn, source, "epoch", &tables, &initial)
            .await
            .unwrap();
        assert_eq!(target.table_positions().await.unwrap()[0].position, last);
        drop(target);
        use sqlx_core::sql_str::AssertSqlSafe;
        for (view, expected) in [(bad_view, "rebased"), (good_view, "healthy")] {
            let rows: Vec<(i64, String)> =
                sqlx::query_as(AssertSqlSafe(format!("SELECT id,payload FROM {view}")))
                    .fetch_all(&mut conn)
                    .await
                    .unwrap();
            assert_eq!(rows, vec![(1, expected.into())]);
        }
    }
    conn.close().await.unwrap();
}
