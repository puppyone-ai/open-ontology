//! Lane F document-source dispatch. Never silently downgrade MongoDB jsonb.
use super::*;

pub(super) async fn run(
    source: &Source,
    tables: &[TableRef],
    target: &TargetConfig,
    store: &SecretStore,
    budget: &ReadBudget,
    opts: &BTreeMap<String, String>,
) -> Result<(), String> {
    if source.engine == "mongodb" {
        // The shared target must first preserve the document metadata and
        // atomically persist an opaque resume token with its row changes.
        return Err("mongodb_target_jsonb_and_resume_token_integration_required".into());
    }
    #[cfg(feature = "document-sources")]
    return search::run(source, tables, target, store, budget, opts).await;
    #[cfg(not(feature = "document-sources"))]
    {
        let _ = (tables, target, store, budget, opts);
        Err("document_sources_feature_required".into())
    }
}

#[cfg(feature = "document-sources")]
mod search {
    use super::*;
    use oo_connectors::opensearch::{Index, Product, SearchConfig, SearchEngine};

    impl epoch::Epoch for SearchEngine {
        async fn close_source(&mut self) {
            // run() uses an explicit close on snapshots; the generic C5 close
            // trait has no error channel. PIT leases also expire after five minutes.
            if self.close().await.is_err() {
                eprintln!("search_pit_cleanup_failed");
            }
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

    pub(super) async fn run(
        source: &Source,
        tables: &[TableRef],
        target: &TargetConfig,
        store: &SecretStore,
        budget: &ReadBudget,
        opts: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        if opts.contains_key("--sync") || opts.contains_key("--oracle-worker") {
            return Err("search_snapshot_and_c5_only".into());
        }
        // No hidden schema inference: a caller supplies stable keyword ranges
        // and the desired top-level scalar projections, not credentials.
        let path =
            std::env::var("OO_SEARCH_INDEX_CONFIG").map_err(|_| "search_index_config_required")?;
        let indices: BTreeMap<String, Index> =
            serde_json::from_slice(&read(&path)?).map_err(|_| "search_index_config")?;
        if indices.len() != tables.len()
            || tables
                .iter()
                .any(|t| t.schema != source.database || !indices.contains_key(&t.name))
        {
            return Err("search_index_config_scope".into());
        }
        let ca_file = match &source.tls_ca {
            None => None,
            Some(ca) => {
                store.read(&ca.secret_ref)?;
                let root = opts
                    .get("--secrets-dir")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                            .join(".config/open-ontology/secrets")
                    });
                Some(root.join(&ca.secret_ref))
            }
        };
        let product = match source.engine.as_str() {
            "opensearch" => Product::Opensearch,
            "elasticsearch" => Product::Elasticsearch,
            _ => return Err("search_product_required".into()),
        };
        let mut engine = SearchEngine::new(
            SearchConfig {
                product,
                host: source.host.clone(),
                port: source.port,
                user: source.user.clone(),
                ca_file,
                schema: source.database.clone(),
                indices,
            },
            zeroize::Zeroizing::new(store.read(&source.password.secret_ref)?),
            budget.clone(),
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
        let closed = engine.close().await.map_err(|e| e.to_string());
        eprintln!(
            "{}",
            serde_json::json!({"protocol":"oo.load.resources/v1",
            "source":source.id,"resources":engine.stats,"document_storage":"text",
            "digest_execution":"bounded_client_scan","pit_cleanup":closed.is_ok()})
        );
        result.and(closed)
    }
}
