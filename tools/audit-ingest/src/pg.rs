//! One transaction for the event and its receipt. No UPDATE.

use std::fs;
use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use rustls::{ClientConfig, RootCertStore};
use tokio_postgres::types::ToSql;
use tokio_postgres::Client;
use zeroize::Zeroize;

use crate::{hex_encode, IngestConfig, IngestError, PreparedEvent};

pub struct PgStore {
    client: Client,
}

impl PgStore {
    pub async fn connect(config: &IngestConfig) -> Result<Self, IngestError> {
        let mut password = read_secret(&config.pg_password_file)?;
        let ca = fs::read(&config.pg_ca_file).map_err(IngestError::Io)?;
        let tls = pg_tls(&ca)?;
        let mut pg = tokio_postgres::Config::new();
        pg.user(&config.pg_user);
        pg.password(&password);
        pg.host(&config.pg_host);
        pg.port(config.pg_port);
        pg.dbname(&config.pg_db);
        pg.ssl_mode(tokio_postgres::config::SslMode::Require);
        pg.application_name("oo-audit-ingest");
        let connected = pg.connect(tls).await;
        password.zeroize();
        let (client, connection) = match connected {
            Ok(pair) => pair,
            Err(err) => return Err(db_error(&err)),
        };
        tokio::spawn(async move {
            if let Err(err) = connection.await {
                eprintln!("pg={}", db_label(&err));
            }
        });
        Ok(Self { client })
    }

    pub async fn insert_event(&mut self, event: &PreparedEvent) -> Result<String, IngestError> {
        let transaction = self.client.transaction().await.map_err(|err| db_error(&err))?;
        let seq = i64::try_from(event.event_seq).map_err(|_| IngestError::Bound)?;
        let hash: &[u8] = &event.payload_hash;
        let payload: &[u8] = &event.payload;
        let insert_params: [&(dyn ToSql + Sync); 7] = [
            &event.spool_id,
            &event.request_id,
            &seq,
            &hash,
            &payload,
            &event.project,
            &event.kind,
        ];
        let inserted = transaction
            .query(
                "INSERT INTO oo_audit_ingest.event \
                 (spool_id, request_id, event_seq, payload_hash, payload, project, kind) \
                 VALUES (($1::text)::uuid, ($2::text)::uuid, $3::bigint, $4::bytea, $5::bytea, $6::text, $7::text) \
                 ON CONFLICT (spool_id, request_id, event_seq) DO NOTHING \
                 RETURNING spool_id::text",
                &insert_params,
            )
            .await
            .map_err(|err| db_error(&err))?;
        let receipt = if inserted.is_empty() {
            existing_receipt(&transaction, event, seq).await?
        } else {
            insert_receipt(&transaction, event, seq).await?
        };
        transaction.commit().await.map_err(|err| db_error(&err))?;
        Ok(receipt)
    }
}

async fn existing_receipt(
    transaction: &tokio_postgres::Transaction<'_>,
    event: &PreparedEvent,
    seq: i64,
) -> Result<String, IngestError> {
    let rows = transaction
        .query(
            "SELECT encode(e.payload_hash, 'hex'), r.receipt_id::text \
             FROM oo_audit_ingest.event e \
             LEFT JOIN oo_audit_ingest.receipt r \
               ON r.spool_id = e.spool_id AND r.request_id = e.request_id AND r.event_seq = e.event_seq \
             WHERE e.spool_id = ($1::text)::uuid AND e.request_id = ($2::text)::uuid AND e.event_seq = $3::bigint",
            &[&event.spool_id, &event.request_id, &seq],
        )
        .await
        .map_err(|err| db_error(&err))?;
    if rows.len() != 1 {
        return Err(IngestError::Db);
    }
    let stored: String = rows[0].try_get(0).map_err(|_| IngestError::Db)?;
    if stored != hex_encode(&event.payload_hash) {
        return Err(IngestError::Conflict);
    }
    let existing: Option<String> = rows[0].try_get(1).map_err(|_| IngestError::Db)?;
    if let Some(receipt) = existing {
        Ok(receipt)
    } else {
        insert_receipt(transaction, event, seq).await
    }
}

async fn insert_receipt(
    transaction: &tokio_postgres::Transaction<'_>,
    event: &PreparedEvent,
    seq: i64,
) -> Result<String, IngestError> {
    let hash: &[u8] = &event.payload_hash;
    let rows = transaction
        .query(
            "INSERT INTO oo_audit_ingest.receipt \
             (spool_id, request_id, event_seq, payload_hash, receipt_id) \
             VALUES (($1::text)::uuid, ($2::text)::uuid, $3::bigint, $4::bytea, gen_random_uuid()) \
             RETURNING receipt_id::text",
            &[&event.spool_id, &event.request_id, &seq, &hash],
        )
        .await
        .map_err(|err| db_error(&err))?;
    if rows.len() != 1 {
        return Err(IngestError::Db);
    }
    rows[0].try_get(0).map_err(|_| IngestError::Db)
}

fn pg_tls(ca_pem: &[u8]) -> Result<tokio_postgres_rustls::MakeRustlsConnect, IngestError> {
    let mut roots = RootCertStore::empty();
    for item in CertificateDer::pem_slice_iter(ca_pem) {
        roots.add(item.map_err(|_| IngestError::Tls)?).map_err(|_| IngestError::Tls)?;
    }
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| IngestError::Tls)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(tokio_postgres_rustls::MakeRustlsConnect::new(config))
}

fn read_secret(path: &std::path::Path) -> Result<String, IngestError> {
    let mut raw = fs::read(path).map_err(IngestError::Io)?;
    while raw.last().is_some_and(|byte| *byte == b'\n' || *byte == b'\r') {
        raw.pop();
    }
    String::from_utf8(raw).map_err(|_| IngestError::Bound)
}

fn db_error(err: &tokio_postgres::Error) -> IngestError {
    match err.code() {
        Some(code) => eprintln!("sqlstate={}", code.code()),
        None => eprintln!("sqlstate=none"),
    }
    eprintln!("db={}", db_label(err));
    IngestError::Db
}

fn db_label(err: &tokio_postgres::Error) -> String {
    let mut out = String::new();
    if err.is_closed() {
        out.push_str("closed ");
    }
    for word in err.to_string().split_whitespace() {
        let hexish = word.chars().filter(|char| char.is_ascii_hexdigit()).count() > 16;
        if hexish || word.to_ascii_lowercase().contains("pass") {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
        if out.len() > 140 {
            break;
        }
    }
    out
}
