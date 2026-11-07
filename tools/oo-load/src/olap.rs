//! ClickHouse, Doris and StarRocks arms (S4-F-01): snapshot, and partition
//! refresh through the shared `--c5` range cycle. There is no log CDC, so
//! `--sync` fails with a closed code. Source TLS uses `tls_ca`; without it
//! only a literal loopback host is accepted, and the transport is reported.

use super::*;
use oo_connectors::olap::{OlapConfig, OlapEngine, OlapTls};

pub(super) fn family(name: &str) -> Option<EngineFamily> {
    match name {
        "clickhouse" => Some(EngineFamily::Clickhouse),
        "doris" => Some(EngineFamily::Doris),
        "starrocks" => Some(EngineFamily::Starrocks),
        _ => None,
    }
}

fn no_c1(family: EngineFamily) -> &'static str {
    match family {
        EngineFamily::Clickhouse => "clickhouse_c1_unsupported_use_c5",
        EngineFamily::Doris => "doris_c1_unsupported_use_c5",
        _ => "starrocks_c1_unsupported_use_c5",
    }
}

impl epoch::Epoch for OlapEngine {
    async fn close_source(&mut self) {
        self.close().await;
    }

    async fn epoch(
        &mut self,
        _: &Source,
        _: &SecretStore,
        tables: &[TableRef],
    ) -> Result<String, String> {
        self.source_epoch(tables).await.map_err(|e| e.to_string())
    }
}

fn config(
    source: &Source,
    store: &SecretStore,
    opts: &BTreeMap<String, String>,
) -> Result<(OlapConfig, &'static str), String> {
    let tls = match &source.tls_ca {
        Some(ca) => {
            // Validate the owner-private reference before passing its path.
            store.read(&ca.secret_ref)?;
            let root = opts
                .get("--secrets-dir")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                        .join(".config/open-ontology/secrets")
                });
            Some(OlapTls {
                ca_file: root.join(&ca.secret_ref),
                server_name: None,
            })
        }
        None if matches!(source.host.as_str(), "127.0.0.1" | "::1") => None,
        None => return Err("source_tls_required".into()),
    };
    let transport = if tls.is_some() {
        "tls"
    } else {
        "plaintext_loopback"
    };
    Ok((
        OlapConfig {
            host: source.host.clone(),
            port: source.port,
            database: source.database.clone(),
            user: source.user.clone(),
            tls,
            connect_timeout_ms: 10_000,
        },
        transport,
    ))
}

pub(super) async fn run(
    engine: &str,
    source: &Source,
    tables: &[TableRef],
    target: &TargetConfig,
    store: &SecretStore,
    budget: &ReadBudget,
    opts: &BTreeMap<String, String>,
) -> Result<(), String> {
    let family = family(engine).ok_or("explicit_supported_engine_required")?;
    if opts.contains_key("--sync") {
        return Err(no_c1(family).into());
    }
    if opts.contains_key("--oracle-worker") {
        return Err("arguments".into());
    }
    let (config, transport) = config(source, store, opts)?;
    let mut engine = OlapEngine::new(
        family,
        config,
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
    eprintln!(
        "{}",
        serde_json::json!({
            "protocol":"oo.load.resources/v1", "source":source.id, "transport":transport,
            "resources":engine.stats(),
            "excluded_columns": tables.iter().map(|t| (t.to_string(), engine.excluded_columns(t))).collect::<BTreeMap<_,_>>()
        })
    );
    engine.close().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn olap_dispatch_is_explicit_and_refuses_c1() {
        assert_eq!(family("clickhouse"), Some(EngineFamily::Clickhouse));
        assert_eq!(family("doris"), Some(EngineFamily::Doris));
        assert_eq!(family("starrocks"), Some(EngineFamily::Starrocks));
        assert_eq!(family("mysql"), None);
        assert_eq!(no_c1(EngineFamily::Doris), "doris_c1_unsupported_use_c5");
    }

    #[test]
    fn plaintext_only_on_literal_loopback() {
        let source = |host: &str| -> Source {
            serde_json::from_value(serde_json::json!({
                "id": "olap", "engine": "clickhouse", "host": host, "port": 9004,
                "database": "fixture", "user": "reader", "password": {"secret_ref": "pw"},
                "scope": {"include": ["fixture.events"], "exclude": []}
            }))
            .unwrap()
        };
        let dir = std::env::temp_dir().join(format!("oo-load-olap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let store = SecretStore::open(dir.clone()).unwrap();
        let opts = BTreeMap::new();
        let (local, transport) = config(&source("127.0.0.1"), &store, &opts).unwrap();
        assert!(local.tls.is_none());
        assert_eq!(transport, "plaintext_loopback");
        assert_eq!(
            config(&source("db.internal"), &store, &opts)
                .err()
                .as_deref(),
            Some("source_tls_required")
        );
        assert_eq!(
            config(&source("localhost"), &store, &opts).err().as_deref(),
            Some("source_tls_required")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
