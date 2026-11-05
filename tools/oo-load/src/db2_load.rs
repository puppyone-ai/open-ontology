//! Source-only Db2 worker dispatch. Snapshot publication uses the shared target.
use super::*;
use oo_connectors::db2::{ConnectConfig, Db2Engine};

impl epoch::Epoch for Db2Engine {
    async fn epoch(
        &mut self,
        _: &Source,
        _: &SecretStore,
        tables: &[TableRef],
    ) -> Result<String, String> {
        if tables.len() != 1 {
            return Err("db2_lineage_requires_one_table".into());
        }
        self.table_lineage(&tables[0])
            .await
            .map_err(|e| e.to_string())
    }

    fn adopt_plan(&mut self, plan: &engine::TableChunkPlan) -> Result<(), String> {
        self.adopt_keyset_plan(plan).map_err(|e| e.to_string())
    }
}

pub(super) async fn run(
    source: &Source,
    tables: &[TableRef],
    target: &TargetConfig,
    store: &SecretStore,
    budget: &ReadBudget,
    opts: &BTreeMap<String, String>,
) -> Result<(), String> {
    if source.ssh.is_some()
        || ["--sync", "--handoff", "--oracle-worker"]
            .iter()
            .any(|key| opts.contains_key(*key))
    {
        return Err("db2_snapshot_c5_only_no_ssh".into());
    }
    let mut config = ConnectConfig::new(
        &source.host,
        source.port,
        &source.database,
        &source.user,
        zeroize::Zeroizing::new(store.read(&source.password.secret_ref)?),
    );
    config.scope = tables.to_vec();
    if source.tls_ca.is_some() {
        // The trusted wrapper mounts this source's caller-supplied CA read-only.
        config.tls_ca = Some("/run/oo-source/ca.pem".into());
    }
    let mut engine = Db2Engine::connect(
        &mut std::process::Command::new(opts.get("--db2-worker").ok_or("db2_worker_required")?),
        &config,
    )
    .map_err(|e| e.to_string())?;
    if let Some(path) = opts.get("--c5") {
        return c5::run(
            engine,
            source,
            tables,
            target,
            store,
            budget,
            serde_json::from_slice(&read(path)?).map_err(|_| "c5_config")?,
        )
        .await;
    }
    let result = run_engine(&mut engine, source, tables, target, store, budget).await;
    eprintln!(
        "{}",
        serde_json::json!({
            "protocol": "oo.load.resources/v1",
            "source": source.id,
            "resources": engine.stats(),
            "driver": "isolated_ibm_cli",
            "consistency": "B",
            "source_time": "unmeasured"
        })
    );
    result
}
