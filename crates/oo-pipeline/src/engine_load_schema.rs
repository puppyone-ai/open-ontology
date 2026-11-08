use super::*;
use oo_connectors::engine::SourceColumnShape;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SealedColumnCatalog {
    pub columns: Vec<SourceColumnShape>,
    pub projection: Vec<String>,
}

impl PgTarget<'_> {
    pub fn published_projection(&mut self, names: Vec<String>) {
        self.projection = Some(names);
    }

    /// DDL can alter IPC metadata without altering any projected value.
    /// Compare multisets, including duplicate rows, before sealing a new range.
    pub async fn staged_matches(&mut self, old: &C5Part, chunk: u32) -> Result<bool> {
        let names = self
            .fields
            .as_ref()
            .ok_or("load_schema")?
            .iter()
            .map(|f| quote(&f.name))
            .collect::<Result<Vec<_>>>()?
            .join(",");
        let sql = format!(
            "SELECT NOT EXISTS(
             (SELECT {names} FROM _oo_load.{} WHERE __oo_chunk=$1
              EXCEPT ALL SELECT {names} FROM _oo_load.{} WHERE __oo_chunk=$1)
             UNION ALL
             (SELECT {names} FROM _oo_load.{} WHERE __oo_chunk=$1
              EXCEPT ALL SELECT {names} FROM _oo_load.{} WHERE __oo_chunk=$1))",
            quote(&self.stage)?,
            quote(&old.stage)?,
            quote(&old.stage)?,
            quote(&self.stage)?,
        );
        sqlx::query_scalar(AssertSqlSafe(sql))
            .bind(i64::from(chunk))
            .fetch_one(&mut *self.conn)
            .await
            .map_err(db)
    }

    /// Set only fresh, native source facts. Unsupported engines keep None.
    pub fn source_columns(&mut self, columns: Option<Vec<SourceColumnShape>>) {
        self.source_columns = columns;
    }

    pub async fn sealed_columns(&mut self) -> Result<Option<SealedColumnCatalog>> {
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT columns::text,projection::text FROM _oo_load.column_catalog WHERE identity=$1",
        )
        .bind(&self.identity)
        .fetch_optional(&mut *self.conn)
        .await
        .map_err(db)?;
        row.map(|(columns, projection)| {
            Ok(SealedColumnCatalog {
                columns: serde_json::from_str(&columns).map_err(|_| "load_column_catalog")?,
                projection: serde_json::from_str(&projection).map_err(|_| "load_column_catalog")?,
            })
        })
        .transpose()
    }

    /// Project decoded row images before staging; never publish newly observed
    /// columns just because a source reader returns SELECT *.
    pub fn project(&self, mut decoded: DecodedStream, names: &[String]) -> Result<DecodedStream> {
        let mut fields = Vec::new();
        let mut columns = Vec::new();
        for name in names {
            let i = decoded
                .fields
                .iter()
                .position(|f| &f.name == name)
                .ok_or("load_schema_drift")?;
            fields.push(decoded.fields[i].clone());
            columns.push(std::mem::take(&mut decoded.columns[i]));
        }
        decoded.fields = fields;
        decoded.columns = columns;
        Ok(decoded)
    }
}

pub(super) async fn seal_columns(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    identity: &str,
    run: &str,
    columns: Option<&[SourceColumnShape]>,
    fields: &[DecodedField],
) -> Result<()> {
    let Some(columns) = columns else {
        // Rebaseline with an unsupported adapter must not keep an old proof.
        sqlx::query("DELETE FROM _oo_load.column_catalog WHERE identity=$1")
            .bind(identity)
            .execute(&mut **tx)
            .await
            .map_err(db)?;
        return Ok(());
    };
    for field in fields {
        if !columns
            .iter()
            .any(|c| c.name == field.name && column(field).is_ok_and(|(_, ty)| ty == c.target_type))
        {
            return Err("load_column_catalog_mismatch");
        }
    }
    let projection: Vec<_> = fields.iter().map(|f| &f.name).collect();
    sqlx::query(
        "INSERT INTO _oo_load.column_catalog(identity,baseline_run,columns,projection)
         VALUES($1,$2,$3::jsonb,$4::jsonb) ON CONFLICT(identity) DO UPDATE
         SET baseline_run=EXCLUDED.baseline_run,columns=EXCLUDED.columns,
         projection=EXCLUDED.projection,sealed_at=clock_timestamp()",
    )
    .bind(identity)
    .bind(run)
    .bind(serde_json::to_string(columns).map_err(|_| "load_column_catalog")?)
    .bind(serde_json::to_string(&projection).map_err(|_| "load_column_catalog")?)
    .execute(&mut **tx)
    .await
    .map_err(db)?;
    Ok(())
}

impl PgTarget<'_> {
    /// Upgrade a pre-0129 table that has no sealed catalog. Only call this after
    /// proving, with the table's legacy lineage, that the source shape is the
    /// one the old fence accepted for the current generation. The catalog is
    /// sealed from the given live facts and the current generation's own
    /// columns, which must all exist with the same target types; otherwise the
    /// table stays `schema_unproven`. `lineage`, when given, rewrites the C5
    /// state from the legacy format (`old`) to the current one (`new`).
    pub async fn adopt_legacy_catalog(
        &mut self,
        run: Option<&str>,
        columns: &[SourceColumnShape],
        lineage: Option<(&str, &str)>,
    ) -> Result<()> {
        let mut tx = self.conn.begin().await.map_err(db)?;
        let current: Option<String> =
            sqlx::query_scalar("SELECT run FROM _oo_load.current WHERE identity=$1 FOR UPDATE")
                .bind(&self.identity)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db)?;
        let run = match (current.as_deref(), run) {
            (Some(current), Some(run)) if current == run => run.to_owned(),
            (Some(current), None) => current.to_owned(),
            _ => return Err("schema_unproven"),
        };
        let run = run.as_str();
        let relation: Option<String> = sqlx::query_scalar(
            "SELECT relation_name FROM _oo_load.generations WHERE identity=$1 AND run=$2",
        )
        .bind(&self.identity)
        .bind(run)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?;
        let relation = relation.ok_or("schema_unproven")?;
        let published: Vec<(String, String)> = sqlx::query_as(
            "SELECT attname::text,format_type(atttypid,atttypmod) FROM pg_attribute
             WHERE attrelid=to_regclass($1) AND attnum>0 AND NOT attisdropped ORDER BY attnum",
        )
        .bind(format!("_oo_load.{}", quote(&relation)?))
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?;
        if !legacy_projection_matches(&published, columns) {
            return Err("schema_unproven");
        }
        let projection: Vec<&String> = published.iter().map(|(name, _)| name).collect();
        let sealed = sqlx::query(
            "INSERT INTO _oo_load.column_catalog(identity,baseline_run,columns,projection)
             VALUES($1,$2,$3::jsonb,$4::jsonb) ON CONFLICT(identity) DO NOTHING",
        )
        .bind(&self.identity)
        .bind(run)
        .bind(serde_json::to_string(columns).map_err(|_| "load_column_catalog")?)
        .bind(serde_json::to_string(&projection).map_err(|_| "load_column_catalog")?)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        if sealed.rows_affected() != 1 {
            return Err("load_column_catalog");
        }
        if let Some((old, new)) = lineage {
            let moved = sqlx::query(
                "UPDATE _oo_load.c5_state SET state=jsonb_set(state,'{lineage}',to_jsonb($4::text))
                 WHERE identity=$1 AND run=$2 AND state->>'lineage'=$3",
            )
            .bind(&self.identity)
            .bind(run)
            .bind(old)
            .bind(new)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
            if moved.rows_affected() != 1 {
                return Err("c5_rebaseline_required_lineage");
            }
        }
        tx.commit().await.map_err(db)
    }

    /// Rewrite only a proven legacy C5 lineage when the catalog already exists.
    pub async fn adopt_legacy_lineage(&mut self, run: &str, old: &str, new: &str) -> Result<()> {
        let moved = sqlx::query(
            "UPDATE _oo_load.c5_state SET state=jsonb_set(state,'{lineage}',to_jsonb($4::text))
             WHERE identity=$1 AND run=$2 AND state->>'lineage'=$3
             AND EXISTS(SELECT 1 FROM _oo_load.current c WHERE c.identity=$1 AND c.run=$2)",
        )
        .bind(&self.identity)
        .bind(run)
        .bind(old)
        .bind(new)
        .execute(&mut *self.conn)
        .await
        .map_err(db)?;
        if moved.rows_affected() != 1 {
            return Err("c5_rebaseline_required_lineage");
        }
        Ok(())
    }
}

/// Every published column of the generation exists in the live facts with the
/// same target type (the comparison `seal_columns` applies to a fresh seal).
fn legacy_projection_matches(published: &[(String, String)], live: &[SourceColumnShape]) -> bool {
    !published.is_empty()
        && published.iter().all(|(name, ty)| {
            let ty = match ty.as_str() {
                "timestamp without time zone" => "timestamp",
                "timestamp with time zone" => "timestamptz",
                other => other,
            };
            live.iter().filter(|c| &c.name == name).count() == 1
                && live.iter().any(|c| &c.name == name && c.target_type == ty)
        })
}

#[cfg(test)]
mod legacy_tests {
    use super::*;

    fn column(name: &str, target: &str) -> SourceColumnShape {
        SourceColumnShape {
            name: name.into(),
            ordinal: 1,
            source_type: target.into(),
            target_type: target.into(),
            nullable: false,
            charset: None,
            collation: None,
            primary_key_ordinal: None,
            unique_key_ordinals: Default::default(),
        }
    }

    #[test]
    fn legacy_seal_requires_every_published_column_with_its_type() {
        let published = vec![
            ("ID".to_string(), "bigint".to_string()),
            ("at".to_string(), "timestamp with time zone".to_string()),
        ];
        let live = vec![column("ID", "bigint"), column("at", "timestamptz")];
        assert!(legacy_projection_matches(&published, &live));
        let mut extra = live.clone();
        extra.push(column("unpublished", "text"));
        assert!(legacy_projection_matches(&published, &extra));
        assert!(!legacy_projection_matches(&published, &live[..1]));
        assert!(!legacy_projection_matches(
            &published,
            &[column("ID", "integer"), live[1].clone()]
        ));
        assert!(!legacy_projection_matches(
            &published,
            &[column("id", "bigint"), live[1].clone()]
        ));
        assert!(!legacy_projection_matches(&[], &live));
    }
}
