use mysql_async::{Conn, Opts, Params, prelude::Queryable};
use mysql_preview::c5plus::*;

struct Current(Vec<TypedRow>);
impl TargetRows for Current {
    fn current_rows(&self, _: &str) -> Result<Vec<TypedRow>> {
        Ok(self.0.clone())
    }
}

#[tokio::test]
async fn owned_mysql57_digest_and_windows() -> Result<()> {
    let mut admin = Conn::new(Opts::from_url("mysql://root:fixture-only@127.0.0.1:23361")?).await?;
    let version: Option<(String, u8)> = admin.query_first("SELECT VERSION(),@@log_bin").await?;
    let (version, bin) = version.unwrap();
    assert!(version.starts_with("5.7."));
    assert_eq!(bin, 0);
    admin
        .query_drop("CREATE DATABASE IF NOT EXISTS c5fixture")
        .await?;
    admin
        .query_drop("CREATE USER IF NOT EXISTS 'reader'@'%' IDENTIFIED BY 'fixture-reader'")
        .await?;
    admin
        .query_drop("GRANT SELECT,LOCK TABLES,SHOW VIEW ON c5fixture.* TO 'reader'@'%'")
        .await?;
    admin
        .query_drop("GRANT PROCESS,REPLICATION SLAVE,REPLICATION CLIENT ON *.* TO 'reader'@'%'")
        .await?;
    for table in ["ints", "strings"] {
        admin
            .query_drop(format!("DROP TABLE IF EXISTS c5fixture.{table}"))
            .await?;
        let key = if table == "ints" {
            "BIGINT"
        } else {
            "VARCHAR(40) COLLATE utf8mb4_unicode_ci"
        };
        admin.query_drop(format!("CREATE TABLE c5fixture.{table} (id {key} PRIMARY KEY,v VARCHAR(40),biz DATETIME(6),d DECIMAL(20,6),f DOUBLE,b BLOB,INDEX(biz)) DEFAULT CHARSET=utf8mb4")).await?;
        admin.query_drop(format!("INSERT INTO c5fixture.{table} VALUES ('1','one','2020-01-01',1.230000,0.1,X'00FF'),('2','','2020-01-01',NULL,1.23,X'23'),('3',NULL,NOW(6),0,0,NULL)")).await?;
    }
    admin.query_drop("SET GLOBAL log_output='TABLE'").await?;
    admin.query_drop("SET GLOBAL general_log=ON").await?;
    let conn = Conn::new(Opts::from_url(
        "mysql://reader:fixture-reader@127.0.0.1:23361/c5fixture",
    )?)
    .await?;
    let mut scanner = Scanner::new(conn, Budget::default(), Window::parse("00:00-00:00")?);
    let mut busy = Conn::new(Opts::from_url("mysql://root:fixture-only@127.0.0.1:23361")?).await?;
    let busy_task = tokio::spawn(async move {
        busy.query_drop("SELECT SLEEP(1)").await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    scanner.budget.max_threads = 1;
    let probe = digest_sql("c5fixture", "ints", &["id"], &["id"], "1", 5000)?;
    scanner.digest(&probe, Params::Empty).await?;
    busy_task.await?;
    assert!(scanner.paused >= std::time::Duration::from_millis(300));
    scanner.budget.max_threads = 8;
    let resume_path = std::env::temp_dir().join(format!(
        "oo-wf-c5plus-live-progress-{}.json",
        std::process::id()
    ));
    let mut progress = ScanProgress {
        table: "c5fixture.ints".into(),
        previous: vec![],
        completed: vec![],
        planned: vec![
            ("c0".into(), serde_json::json!([0, 1])),
            ("c1".into(), serde_json::json!([1, 9])),
        ],
    };
    let resume_sql = digest_sql(
        "c5fixture",
        "ints",
        &["id"],
        &["id", "v"],
        "id > ? AND id <= ?",
        5000,
    )?;
    let (digest, _) = scanner
        .digest(&resume_sql, Params::Positional(vec![0.into(), 1.into()]))
        .await?;
    progress.checkpoint_applied(
        Chunk {
            chunk_id: "c0".into(),
            bound: serde_json::json!([0, 1]),
            rows: digest[0],
            digest: [digest[1], digest[2]],
            scanned_at: 1,
        },
        &resume_path,
    )?;
    scanner
        .digest(&resume_sql, Params::Positional(vec![1.into(), 9.into()]))
        .await?;
    drop(scanner);
    let conn = Conn::new(Opts::from_url(
        "mysql://reader:fixture-reader@127.0.0.1:23361/c5fixture",
    )?)
    .await?;
    let mut scanner = Scanner::new(conn, Budget::default(), Window::parse("00:00-00:00")?);
    let mut progress = ScanProgress::load(&resume_path)?;
    assert_eq!(progress.next_pending().unwrap().0, "c1");
    let (digest, _) = scanner
        .digest(&resume_sql, Params::Positional(vec![1.into(), 9.into()]))
        .await?;
    progress.checkpoint_applied(
        Chunk {
            chunk_id: "c1".into(),
            bound: serde_json::json!([1, 9]),
            rows: digest[0],
            digest: [digest[1], digest[2]],
            scanned_at: 2,
        },
        &resume_path,
    )?;
    assert_eq!(progress.changes()?.new.len(), 2);
    std::fs::remove_file(resume_path)?;
    for table in ["ints", "strings"] {
        let columns = ["id", "v", "biz", "d", "f", "b"];
        let schema = scanner.schema("c5fixture", table).await?;
        use oo_connectors::mysql::map::StoreKind;
        let key_kind = if table == "ints" {
            StoreKind::Int { width: 8 }
        } else {
            StoreKind::Text { limit: None }
        };
        let typed = scanner
            .typed_rows(
                "c5fixture",
                table,
                &[
                    ("id", key_kind, false),
                    ("v", StoreKind::Text { limit: None }, true),
                    ("biz", StoreKind::TimestampNaive, true),
                    (
                        "d",
                        StoreKind::Numeric {
                            precision: 20,
                            scale: 6,
                        },
                        true,
                    ),
                    ("f", StoreKind::Float8, true),
                    ("b", StoreKind::Bytes, true),
                ],
                &[0],
                "1",
                Params::Empty,
            )
            .await?;
        assert_eq!(typed.len(), 3);
        assert_eq!(typed[0].values.len(), 6);

        let diff_columns = [
            ("id", key_kind, false),
            ("v", StoreKind::Text { limit: None }, true),
        ];
        let initial = scanner
            .typed_rows("c5fixture", table, &diff_columns, &[0], "1", Params::Empty)
            .await?;
        admin
            .query_drop(format!(
                "UPDATE c5fixture.{table} SET v='intermediate' WHERE id='2'"
            ))
            .await?;
        admin
            .query_drop(format!(
                "UPDATE c5fixture.{table} SET v='final' WHERE id='2'"
            ))
            .await?;
        let final_rows = scanner
            .typed_rows("c5fixture", table, &diff_columns, &[0], "1", Params::Empty)
            .await?;
        let changes = reconcile(&final_rows, &Current(initial), "all", 1)?;
        assert_eq!(changes.len(), 1);
        assert!(matches!(&changes[0], Mutation::Update(_)));

        let sql = digest_sql(
            "c5fixture",
            table,
            &["id"],
            &columns,
            "id > ? AND id <= ?",
            5000,
        )?;
        let params = Params::Positional(vec!["0".into(), "9".into()]);
        let crc: Option<(u64, u64)> = admin
            .query_first("SELECT CRC32('13388230944202771988'),CRC32('14997315743545178887')")
            .await?;
        let (a, b) = crc.unwrap();
        assert_eq!(a, b);
        admin
            .query_drop(format!(
                "UPDATE c5fixture.{table} SET v='13388230944202771988' WHERE id='1'"
            ))
            .await?;
        let crc_before = scanner.digest(&sql, params.clone()).await?.0;
        admin
            .query_drop(format!(
                "UPDATE c5fixture.{table} SET v='14997315743545178887' WHERE id='1'"
            ))
            .await?;
        assert_ne!(crc_before, scanner.digest(&sql, params.clone()).await?.0);
        admin
            .query_drop(format!("UPDATE c5fixture.{table} SET v='one' WHERE id='1'"))
            .await?;
        let first = scanner.digest(&sql, params.clone()).await?.0;
        assert_eq!(first, scanner.digest(&sql, params.clone()).await?.0);
        admin.query_drop(format!("UPDATE c5fixture.{table} SET v=CASE id WHEN '1' THEN '' WHEN '2' THEN 'one' ELSE v END")).await?;
        assert_ne!(first, scanner.digest(&sql, params.clone()).await?.0);
        let (p, values) = recent_predicate("biz", "2030-01-01".into(), "2025-01-01".into())?;
        assert_eq!(
            scanner
                .rows("c5fixture", table, &columns, &p, values)
                .await?
                .len(),
            1
        );
        let old_before = scanner.digest(&sql, params.clone()).await?.0;
        admin
            .query_drop(format!(
                "UPDATE c5fixture.{table} SET v='old updated' WHERE id='1'"
            ))
            .await?;
        assert_ne!(old_before, scanner.digest(&sql, params.clone()).await?.0);
        let (window, window_params) =
            recent_predicate("biz", "2030-01-01".into(), "2025-01-01".into())?;
        assert_eq!(
            scanner
                .rows("c5fixture", table, &columns, &window, window_params.clone())
                .await?
                .len(),
            1
        );
        admin
            .query_drop(format!(
                "INSERT INTO c5fixture.{table} VALUES ('4','new',NOW(6),1,0,NULL)"
            ))
            .await?;
        assert_eq!(
            scanner
                .rows("c5fixture", table, &columns, &window, window_params.clone())
                .await?
                .len(),
            2
        );
        admin.query_drop("START TRANSACTION").await?;
        admin
            .query_drop(format!(
                "INSERT INTO c5fixture.{table} VALUES ('5','late','2026-01-01',1,0,NULL)"
            ))
            .await?;
        assert_eq!(
            scanner
                .rows("c5fixture", table, &columns, &window, window_params.clone())
                .await?
                .len(),
            2
        );
        admin.query_drop("COMMIT").await?;
        assert_eq!(
            scanner
                .rows("c5fixture", table, &columns, &window, window_params.clone())
                .await?
                .len(),
            3
        );
        let (late_no_overlap, late_params) =
            recent_predicate("biz", "2026-01-03".into(), "2026-01-02".into())?;
        assert_eq!(
            scanner
                .rows("c5fixture", table, &columns, &late_no_overlap, late_params)
                .await?
                .len(),
            0
        );
        let (late_overlap, late_params) =
            recent_predicate("biz", "2026-01-03".into(), "2026-01-01".into())?;
        assert_eq!(
            scanner
                .rows("c5fixture", table, &columns, &late_overlap, late_params)
                .await?
                .len(),
            1
        );
        if table == "strings" {
            admin
                .query_drop("INSERT INTO c5fixture.strings VALUES ('A1','case',NOW(6),1,0,NULL)")
                .await?;
            assert!(
                admin
                    .query_drop(
                        "INSERT INTO c5fixture.strings VALUES ('a1','case',NOW(6),1,0,NULL)"
                    )
                    .await
                    .is_err()
            );
            assert!(
                admin
                    .query_drop(
                        "INSERT INTO c5fixture.strings VALUES ('A1 ','case',NOW(6),1,0,NULL)"
                    )
                    .await
                    .is_err()
            );
            let matched = scanner
                .rows(
                    "c5fixture",
                    table,
                    &columns,
                    "id >= ? AND id <= ?",
                    Params::Positional(vec!["a1".into(), "A1 ".into()]),
                )
                .await?;
            assert_eq!(matched.len(), 1);
        }
        let target_before_delete = scanner
            .typed_rows("c5fixture", table, &diff_columns, &[0], "1", Params::Empty)
            .await?;
        let before = scanner.digest(&sql, params.clone()).await?.0;
        admin
            .query_drop(format!("DELETE FROM c5fixture.{table} WHERE id='1'"))
            .await?;
        assert_ne!(before, scanner.digest(&sql, params.clone()).await?.0);
        let deleted = scanner
            .typed_rows("c5fixture", table, &diff_columns, &[0], "1", Params::Empty)
            .await?;
        let deletion = reconcile(&deleted, &Current(target_before_delete), "all", 7)?;
        assert_eq!(deletion.len(), 1);
        assert!(matches!(
            &deletion[0],
            Mutation::Delete {
                tombstone_generation: 7,
                ..
            }
        ));
        admin
            .query_drop(format!(
                "INSERT INTO c5fixture.{table} VALUES ('1','reused',NOW(6),1,0,NULL)"
            ))
            .await?;
        let reused = scanner
            .typed_rows("c5fixture", table, &diff_columns, &[0], "1", Params::Empty)
            .await?;
        let reuse = reconcile(&reused, &Current(deleted), "all", 8)?;
        assert_eq!(reuse.len(), 1);
        assert!(matches!(&reuse[0], Mutation::Insert(_)));
        admin
            .query_drop(format!(
                "ALTER TABLE c5fixture.{table} ADD COLUMN extra INT"
            ))
            .await?;
        assert_eq!(
            scanner
                .check_schema("c5fixture", table, &schema)
                .await
                .unwrap_err()
                .to_string(),
            "SCHEMA_DRIFT"
        );
    }
    let audited:Option<u64>=admin.query_first("SELECT COUNT(*) FROM mysql.general_log WHERE user_host LIKE 'reader[reader]%' AND command_type IN ('Query','Execute','Prepare')").await?;
    assert!(audited.unwrap() > 20);
    println!("reader_audited_statements={}", audited.unwrap());
    let prohibited:Option<u64>=admin.query_first("SELECT COUNT(*) FROM mysql.general_log WHERE user_host LIKE 'reader[reader]%' AND command_type IN ('Query','Execute','Prepare') AND NOT (argument LIKE 'SELECT%' OR argument LIKE 'SHOW%' OR argument LIKE 'START TRANSACTION%' OR argument LIKE 'COMMIT%' OR argument LIKE 'SET TRANSACTION READ ONLY%' OR argument LIKE 'SET NAMES%' OR argument LIKE 'SET character_set%')").await?;
    assert_eq!(prohibited, Some(0));
    admin.query_drop("SET GLOBAL general_log=OFF").await?;
    admin.disconnect().await?;
    Ok(())
}
