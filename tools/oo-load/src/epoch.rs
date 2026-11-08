use super::*;

pub(super) trait Epoch: SourceEngine {
    async fn close_source(&mut self) {}

    async fn epoch(
        &mut self,
        source: &Source,
        store: &SecretStore,
        tables: &[TableRef],
    ) -> Result<String, String>;

    /// Let engines that only read chunks they planned read the durable C5
    /// partition (after `plan_chunks` of the same key and strategy).
    fn adopt_plan(&mut self, _plan: &engine::TableChunkPlan) -> Result<(), String> {
        Ok(())
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
        self.close_idle_session().await.map_err(|e| e.to_string())?;
        let mut control = source_control_connection(source, store).await?;
        let result = sync::lineage(&mut control, tables)
            .await
            .map_err(String::from);
        control.close().await.map_err(|_| "epoch_source_close")?;
        // The column catalog is compared by the shared projection classifier.
        // Keep instance/timeline/database and relation identity in this fence.
        let mut value: serde_json::Value =
            serde_json::from_str(&result?).map_err(|_| "epoch_shape")?;
        if let Some(relations) = value.get_mut(1).and_then(|v| v.as_array_mut()) {
            for relation in relations {
                relation.as_array_mut().ok_or("epoch_shape")?.truncate(2);
            }
        } else {
            return Err("epoch_shape".into());
        }
        serde_json::to_string(&value).map_err(|_| "epoch_shape".into())
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
}

impl Epoch for oo_connectors::oracle::engine::OracleEngine {
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
    fn legacy_shape_suffix_is_not_part_of_the_handoff_epoch() {
        assert_eq!(epoch_part("uuid#shape:s1"), "uuid");
        assert_eq!(epoch_part("pg"), "pg");
    }
}
