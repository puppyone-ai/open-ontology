//! Source-only DM8 worker dispatch. Snapshot publication uses the shared target.
use super::*;
use oo_connectors::dm8::{ConnectConfig, Dm8Engine};

impl epoch::Epoch for Dm8Engine {
    async fn epoch(
        &mut self,
        _: &Source,
        _: &SecretStore,
        tables: &[TableRef],
    ) -> Result<String, String> {
        if tables.len() != 1 {
            return Err("dm8_lineage_requires_one_table".into());
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
        || source.tls_ca.is_some()
        || ["--sync", "--handoff", "--oracle-worker", "--db2-worker"]
            .iter()
            .any(|key| opts.contains_key(*key))
    {
        return Err("dm8_loopback_snapshot_c5_only".into());
    }
    let mut config = ConnectConfig::new(
        &source.host,
        source.port,
        &source.user,
        zeroize::Zeroizing::new(store.read(&source.password.secret_ref)?),
    );
    config.scope = tables.to_vec();
    let mut engine = Dm8Engine::connect(
        &mut std::process::Command::new(opts.get("--dm8-worker").ok_or("dm8_worker_required")?),
        &config,
    )
    .map_err(|e| e.to_string())?;
    let result = match opts.get("--c5") {
        Some(path) => {
            let policy = serde_json::from_slice(&read(path)?).map_err(|_| "c5_config")?;
            c5::run_cycle(&mut engine, source, tables, target, store, budget, policy).await
        }
        None => run_engine(&mut engine, source, tables, target, store, budget).await,
    };
    eprintln!(
        "{}",
        serde_json::json!({
            "protocol": "oo.load.resources/v1",
            "source": source.id,
            "resources": engine.stats(),
            "driver": "isolated_dm_dpi",
            "consistency": "B",
            "source_time": "unmeasured"
        })
    );
    result
}
