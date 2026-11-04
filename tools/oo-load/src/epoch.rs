use super::*;

pub(super) trait Epoch: SourceEngine {
    async fn epoch(
        &mut self,
        source: &Source,
        store: &SecretStore,
        tables: &[TableRef],
    ) -> Result<String, String>;
}

impl Epoch for PostgresEngine {
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
