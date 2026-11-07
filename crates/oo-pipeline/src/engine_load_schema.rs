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
