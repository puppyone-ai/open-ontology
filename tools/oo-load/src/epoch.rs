use super::*;

pub(super) trait Epoch: SourceEngine {
    async fn close_source(&mut self) {}

    async fn epoch(
        &mut self,
        source: &Source,
        store: &SecretStore,
        tables: &[TableRef],
    ) -> Result<String, String>;

    /// Catalog fingerprint for engines whose epoch does not cover columns.
    async fn shape(&mut self, _table: &TableRef) -> Result<Option<String>, String> {
        Ok(None)
    }

    /// Let engines that only read chunks they planned read the durable C5
    /// partition (after `plan_chunks` of the same key and strategy).
    fn adopt_plan(&mut self, _plan: &engine::TableChunkPlan) -> Result<(), String> {
        Ok(())
    }
}

/// C5 lineage: instance epoch plus, where separate, the table's catalog shape.
/// Rename/widen/nullability DDL can leave every range digest equal; a changed
/// shape must refuse (`c5_rebaseline_required_lineage`), never publish stale columns.
pub(super) fn c5_lineage(epoch: String, shape: Option<String>) -> String {
    match shape {
        Some(shape) => format!("{epoch}{SHAPE}{shape}"),
        None => epoch,
    }
}

const SHAPE: &str = "#shape:";

/// The instance epoch part of a stored C5 lineage, for C5-to-C1 handoff.
pub(super) fn epoch_part(lineage: &str) -> &str {
    lineage.split(SHAPE).next().unwrap_or(lineage)
}

impl Epoch for PostgresEngine {
    fn adopt_plan(&mut self, plan: &engine::TableChunkPlan) -> Result<(), String> {
        self.adopt_keyset_plan(plan).map_err(|e| e.to_string())
    }

    async fn epoch(
        &mut self,
        source: &Source,
        store: &SecretStore,
        tables: &[TableRef],
    ) -> Result<String, String> {
        let mut control = source_control_connection(source, store).await?;
        let result = sync::lineage(&mut control, tables)
            .await
            .map_err(String::from);
        control.close().await.map_err(|_| "epoch_source_close")?;
        result
    }
}

impl Epoch for oo_connectors::mysql::engine::MysqlEngine {
    async fn close_source(&mut self) {
        self.close().await;
    }

    async fn epoch(
        &mut self,
        _: &Source,
        _: &SecretStore,
        _: &[TableRef],
    ) -> Result<String, String> {
        self.source_epoch().await.map_err(|e| e.to_string())
    }

    async fn shape(&mut self, table: &TableRef) -> Result<Option<String>, String> {
        self.table_shape(table)
            .await
            .map(Some)
            .map_err(|e| e.to_string())
    }
}

impl Epoch for oo_connectors::oracle::engine::OracleEngine {
    async fn shape(&mut self, table: &TableRef) -> Result<Option<String>, String> {
        self.table_shape(table).map(Some).map_err(|e| e.to_string())
    }

    async fn epoch(
        &mut self,
        _: &Source,
        _: &SecretStore,
        _: &[TableRef],
    ) -> Result<String, String> {
        self.source_epoch().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_is_part_of_c5_lineage_but_not_of_the_handoff_epoch() {
        let a = c5_lineage("uuid".into(), Some("s1".into()));
        let b = c5_lineage("uuid".into(), Some("s2".into()));
        assert_ne!(a, b);
        assert_eq!(epoch_part(&a), "uuid");
        assert_eq!(c5_lineage("pg".into(), None), "pg");
        assert_eq!(epoch_part("pg"), "pg");
    }
}
