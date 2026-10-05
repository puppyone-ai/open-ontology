//! `oo-pipeline catalog-pg`.
//!
//! Stdin is one bounded `oo.pipeline.catalog-pg/v1` document. The command
//! opens the configured read-only source, requires `inspect_access` to report
//! `registration_allowed`, and writes the `SourceCatalog` from
//! `PgSession::catalog`. Stdout is that value's serde JSON. The protocol is
//! `oo.source.pgsql/v1`. There is no target, no import, and no wrapper.

use std::io::{self, Write};

use oo_connectors::{PgConnector, SourceCatalog, PROTOCOL};
use oo_core::parse_json_value;
use serde::Deserialize;

use crate::import_cli::{
    self, Stop, build_source, from_source, read_bounded_stdin, read_ca, read_password, source_budget,
};

/// Request protocol. The binary accepts this value and no other.
pub const REQUEST_PROTOCOL: &str = "oo.pipeline.catalog-pg/v1";

/// Read one source request and write the session catalog.
pub async fn catalog_pg() -> Result<(), u8> {
    match execute().await {
        Ok(document) => write_stdout(&document),
        Err(stop) => {
            stop.emit();
            Err(1)
        }
    }
}

async fn execute() -> Result<Vec<u8>, Stop> {
    let text = read_bounded_stdin("catalog input")?;
    let value =
        parse_json_value(&text, REQUEST_PROTOCOL).map_err(|_| Stop::Fixed("invalid input"))?;
    let mut request: Request =
        serde_json::from_value(value).map_err(|_| Stop::Fixed("invalid input"))?;
    if request.protocol != REQUEST_PROTOCOL {
        return Err(Stop::Fixed("invalid version"));
    }
    if request.source.tables.is_empty() {
        return Err(Stop::Fixed("invalid input"));
    }
    let budget = source_budget(request.source.budget.take())?;
    let source_password = read_password(&request.source.password_file, "source password file")?;
    let source_ca = read_ca(&request.source.ca_file, "source ca file")?;
    let config = build_source(&request.source, source_password, &source_ca, budget)?;
    let catalog = read_catalog(config).await?;
    encode(&catalog)
}

async fn read_catalog(config: oo_connectors::PgSourceConfig) -> Result<SourceCatalog, Stop> {
    let mut session = PgConnector::new(config).connect().await.map_err(from_source)?;
    let allowed = session
        .inspect_access()
        .await
        .map_err(from_source)?
        .registration_allowed;
    if !allowed {
        return Err(Stop::Fixed("registration refused"));
    }
    session.catalog().await.map_err(from_source)
}

fn encode(catalog: &SourceCatalog) -> Result<Vec<u8>, Stop> {
    if catalog.protocol != PROTOCOL {
        return Err(Stop::Fixed("catalog output"));
    }
    let bytes = serde_json::to_vec(catalog).map_err(|_| Stop::Fixed("catalog output"))?;
    if bytes.len() > oo_core::limits::DOCUMENT_MAX_BYTES {
        return Err(Stop::Fixed("catalog output"));
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| Stop::Fixed("catalog output"))?;
    let value = parse_json_value(text, PROTOCOL).map_err(|_| Stop::Fixed("catalog output"))?;
    let parsed: SourceCatalog =
        serde_json::from_value(value).map_err(|_| Stop::Fixed("catalog output"))?;
    if parsed != *catalog {
        return Err(Stop::Fixed("catalog output"));
    }
    Ok(bytes)
}

fn write_stdout(document: &[u8]) -> Result<(), u8> {
    match io::stdout().write_all(document) {
        Ok(()) => Ok(()),
        Err(_) => {
            eprintln!("oo-pipeline: catalog output");
            Err(1)
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    protocol: String,
    source: import_cli::SourceIn,
}
