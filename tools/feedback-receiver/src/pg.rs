//! One transaction for a receipt and its review rows. No UPDATE.

use std::fs;
use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use rustls::{ClientConfig, RootCertStore};
use serde_json::{Value, json};
use tokio_postgres::types::ToSql;
use tokio_postgres::Client;
use zeroize::Zeroize;

use crate::config::Config;
use crate::error::RecvError;
use crate::review::{Decoded, Reviewed, StoredCandidate, ack, decode_hex, review_page};

pub struct PgStore {
    client: Client,
}

impl PgStore {
    pub async fn connect(config: &Config) -> Result<Self, RecvError> {
        let mut password = read_secret(&config.pg_password_file)?;
        let ca = fs::read(&config.pg_ca_file).map_err(RecvError::Io)?;
        let tls = pg_tls(&ca)?;
        let mut pg = tokio_postgres::Config::new();
        pg.user(&config.pg_user);
        pg.password(&password);
        pg.host(&config.pg_host);
        pg.port(config.pg_port);
        pg.dbname(&config.pg_db);
        pg.ssl_mode(tokio_postgres::config::SslMode::Require);
        pg.application_name("oo-feedback-receiver");
        let connected = pg.connect(tls).await;
        password.zeroize();
        let (client, connection) = match connected {
            Ok(pair) => pair,
            Err(err) => return Err(db_error(&err)),
        };
        tokio::spawn(async move {
            if let Err(err) = connection.await {
                let _ = db_error(&err);
            }
        });
        client
            .batch_execute("SELECT set_config('search_path', 'pg_catalog', false)")
            .await
            .map_err(|err| db_error(&err))?;
        Ok(Self { client })
    }

    pub async fn prior_ids(&self, project: &str) -> Result<Vec<String>, RecvError> {
        let rows = self
            .client
            .query(
                "SELECT DISTINCT c.candidate_id \
                 FROM oo_feedback_receiver.candidate c \
                 JOIN oo_feedback_receiver.receipt r ON r.receipt_id = c.receipt_id \
                 WHERE r.project = $1::text \
                 ORDER BY c.candidate_id \
                 LIMIT 256",
                &[&project],
            )
            .await
            .map_err(|err| db_error(&err))?;
        let mut ids = Vec::with_capacity(rows.len());
        for row in rows {
            ids.push(row.try_get(0).map_err(|_| RecvError::Db)?);
        }
        Ok(ids)
    }

    pub async fn commit(&mut self, decoded: &Decoded, reviewed: &Reviewed) -> Result<Value, RecvError> {
        let transaction = self.client.transaction().await.map_err(|err| db_error(&err))?;
        let payload: &[u8] = &decoded.payload_hash;
        let catalog = hash_bytes(&reviewed.catalog_sha256)?;
        let relationships = hash_bytes(&reviewed.relationships_sha256)?;
        let profile = hash_bytes(&reviewed.profile_sha256)?;
        let feedback = hash_bytes(&reviewed.feedback_sha256)?;
        let evidence = hash_bytes(&reviewed.evidence_sha256)?;
        let effects = reviewed.effects.to_string();
        let dedup = reviewed.dedup.to_string();
        let drops = reviewed.scope_drops.to_string();
        let outcome = reviewed.outcome.to_string();
        let params: [&(dyn ToSql + Sync); 13] = [
            &decoded.receipt_id,
            &decoded.project,
            &payload,
            &catalog,
            &relationships,
            &profile,
            &feedback,
            &evidence,
            &reviewed.schema_hash,
            &effects,
            &dedup,
            &drops,
            &outcome,
        ];
        let inserted = transaction
            .query(
                "INSERT INTO oo_feedback_receiver.receipt \
                 (receipt_id, project, payload_hash, catalog_sha256, relationships_sha256, \
                  profile_sha256, feedback_sha256, evidence_sha256, schema_hash, \
                  effects, dedup, scope_drops, outcome) \
                 VALUES (($1::text)::uuid, $2::text, $3::bytea, $4::bytea, $5::bytea, \
                         $6::bytea, $7::bytea, $8::bytea, $9::text, \
                         ($10::text)::jsonb, ($11::text)::jsonb, ($12::text)::jsonb, ($13::text)::jsonb) \
                 ON CONFLICT (receipt_id) DO NOTHING \
                 RETURNING receipt_id::text",
                &params,
            )
            .await
            .map_err(|err| db_error(&err))?;
        if inserted.is_empty() {
            let existing = transaction
                .query(
                    "SELECT payload_hash, project \
                     FROM oo_feedback_receiver.receipt \
                     WHERE receipt_id = ($1::text)::uuid",
                    &[&decoded.receipt_id],
                )
                .await
                .map_err(|err| db_error(&err))?;
            transaction.rollback().await.map_err(|err| db_error(&err))?;
            if existing.len() != 1 {
                return Err(RecvError::Db);
            }
            let stored: Vec<u8> = existing[0].try_get(0).map_err(|_| RecvError::Db)?;
            let project: String = existing[0].try_get(1).map_err(|_| RecvError::Db)?;
            if stored.as_slice() != decoded.payload_hash || project != decoded.project {
                return Err(RecvError::Conflict);
            }
            return self.load_ack(&decoded.receipt_id, true).await;
        }
        for (index, item) in reviewed.feedback.iter().enumerate() {
            let ordinal = i32::try_from(index).map_err(|_| RecvError::Producer)?;
            let item_params: [&(dyn ToSql + Sync); 6] = [
                &decoded.receipt_id,
                &ordinal,
                &item.candidate_id,
                &item.kind,
                &item.target,
                &item.text,
            ];
            transaction
                .execute(
                    "INSERT INTO oo_feedback_receiver.feedback_item \
                     (receipt_id, ordinal, candidate_id, kind, target, item_text, untrusted) \
                     VALUES (($1::text)::uuid, $2::integer, $3::text, $4::text, $5::text, $6::text, true)",
                    &item_params,
                )
                .await
                .map_err(|err| db_error(&err))?;
        }
        for candidate in &reviewed.candidates {
            let score = i64::from(candidate.score);
            let source = candidate.source.to_string();
            let candidate_params: [&(dyn ToSql + Sync); 9] = [
                &decoded.receipt_id,
                &candidate.candidate_id,
                &candidate.channel,
                &candidate.key,
                &score,
                &candidate.impact,
                &candidate.uncertainty_millis,
                &candidate.prior_overlap,
                &source,
            ];
            transaction
                .execute(
                    "INSERT INTO oo_feedback_receiver.candidate \
                     (receipt_id, candidate_id, channel, queue_key, score, impact, uncertainty_millis, \
                      trusted_metric, knowledge_verified, prior_overlap, source) \
                     VALUES (($1::text)::uuid, $2::text, $3::text, $4::text, $5::bigint, $6::integer, \
                             $7::integer, false, false, $8::boolean, ($9::text)::jsonb)",
                    &candidate_params,
                )
                .await
                .map_err(|err| db_error(&err))?;
        }
        transaction.commit().await.map_err(|err| db_error(&err))?;
        self.load_ack(&decoded.receipt_id, false).await
    }

    pub async fn list(&self, project: &str) -> Result<Value, RecvError> {
        let count_rows = self
            .client
            .query(
                "SELECT count(*)::bigint \
                 FROM oo_feedback_receiver.candidate c \
                 JOIN oo_feedback_receiver.receipt r ON r.receipt_id = c.receipt_id \
                 WHERE r.project = $1::text",
                &[&project],
            )
            .await
            .map_err(|err| db_error(&err))?;
        if count_rows.len() != 1 {
            return Err(RecvError::Db);
        }
        let count: i64 = count_rows[0].try_get(0).map_err(|_| RecvError::Db)?;
        let rows = self
            .client
            .query(
                "SELECT c.candidate_id, c.channel, c.queue_key, c.score, c.impact, \
                        c.uncertainty_millis, c.prior_overlap, c.source::text \
                 FROM oo_feedback_receiver.candidate c \
                 JOIN oo_feedback_receiver.receipt r ON r.receipt_id = c.receipt_id \
                 WHERE r.project = $1::text \
                 ORDER BY c.score DESC, c.candidate_id ASC, c.receipt_id ASC \
                 LIMIT 256",
                &[&project],
            )
            .await
            .map_err(|err| db_error(&err))?;
        let mut candidates = Vec::with_capacity(rows.len());
        for row in &rows {
            candidates.push(read_candidate(row)?);
        }
        Ok(review_page(project, count, &candidates))
    }

    async fn load_ack(&self, receipt_id: &str, duplicate: bool) -> Result<Value, RecvError> {
        let rows = self
            .client
            .query(
                "SELECT project, encode(payload_hash, 'hex'), effects::text, dedup::text, scope_drops::text \
                 FROM oo_feedback_receiver.receipt \
                 WHERE receipt_id = ($1::text)::uuid",
                &[&receipt_id],
            )
            .await
            .map_err(|err| db_error(&err))?;
        if rows.len() != 1 {
            return Err(RecvError::Db);
        }
        let project: String = rows[0].try_get(0).map_err(|_| RecvError::Db)?;
        let payload_hash: String = rows[0].try_get(1).map_err(|_| RecvError::Db)?;
        let effects = parse_json(&rows[0].try_get::<_, String>(2).map_err(|_| RecvError::Db)?)?;
        let dedup = parse_json(&rows[0].try_get::<_, String>(3).map_err(|_| RecvError::Db)?)?;
        let scope_drops = parse_json(&rows[0].try_get::<_, String>(4).map_err(|_| RecvError::Db)?)?;
        let candidate_rows = self
            .client
            .query(
                "SELECT candidate_id, channel, queue_key, score, impact, uncertainty_millis, \
                        prior_overlap, source::text \
                 FROM oo_feedback_receiver.candidate \
                 WHERE receipt_id = ($1::text)::uuid \
                 ORDER BY score DESC, candidate_id ASC",
                &[&receipt_id],
            )
            .await
            .map_err(|err| db_error(&err))?;
        let mut candidates = Vec::with_capacity(candidate_rows.len());
        for row in &candidate_rows {
            candidates.push(read_candidate(row)?);
        }
        let reviewed = Reviewed {
            catalog_sha256: String::new(),
            relationships_sha256: String::new(),
            profile_sha256: String::new(),
            feedback_sha256: String::new(),
            evidence_sha256: String::new(),
            schema_hash: String::new(),
            effects,
            dedup,
            scope_drops,
            outcome: json!({}),
            feedback: Vec::new(),
            candidates,
        };
        Ok(ack(receipt_id, &payload_hash, &project, duplicate, &reviewed))
    }
}

fn read_candidate(row: &tokio_postgres::Row) -> Result<StoredCandidate, RecvError> {
    let score = i32::try_from(row.try_get::<_, i64>(3).map_err(|_| RecvError::Db)?).map_err(|_| RecvError::Db)?;
    let source = parse_json(&row.try_get::<_, String>(7).map_err(|_| RecvError::Db)?)?;
    Ok(StoredCandidate {
        candidate_id: row.try_get(0).map_err(|_| RecvError::Db)?,
        channel: row.try_get(1).map_err(|_| RecvError::Db)?,
        key: row.try_get(2).map_err(|_| RecvError::Db)?,
        score,
        impact: row.try_get(4).map_err(|_| RecvError::Db)?,
        uncertainty_millis: row.try_get(5).map_err(|_| RecvError::Db)?,
        prior_overlap: row.try_get(6).map_err(|_| RecvError::Db)?,
        source,
    })
}

fn parse_json(text: &str) -> Result<Value, RecvError> {
    serde_json::from_str(text).map_err(|_| RecvError::Db)
}

fn hash_bytes(text: &str) -> Result<Vec<u8>, RecvError> {
    let bytes = decode_hex(text).map_err(|_| RecvError::Producer)?;
    if bytes.len() == 32 {
        Ok(bytes)
    } else {
        Err(RecvError::Producer)
    }
}

fn pg_tls(ca_pem: &[u8]) -> Result<tokio_postgres_rustls::MakeRustlsConnect, RecvError> {
    let mut roots = RootCertStore::empty();
    for item in CertificateDer::pem_slice_iter(ca_pem) {
        roots.add(item.map_err(|_| RecvError::Tls)?).map_err(|_| RecvError::Tls)?;
    }
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| RecvError::Tls)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(tokio_postgres_rustls::MakeRustlsConnect::new(config))
}

fn read_secret(path: &std::path::Path) -> Result<String, RecvError> {
    let mut raw = fs::read(path).map_err(RecvError::Io)?;
    while raw.last().is_some_and(|byte| *byte == b'\n' || *byte == b'\r') {
        raw.pop();
    }
    match String::from_utf8(raw) {
        Ok(text) => Ok(text),
        Err(err) => {
            let mut raw = err.into_bytes();
            raw.zeroize();
            Err(RecvError::Bound)
        }
    }
}

fn db_error(err: &tokio_postgres::Error) -> RecvError {
    if let Some(code) = err.code() {
        eprintln!("sqlstate={}", code.code());
    } else {
        eprintln!("sqlstate=none");
    }
    RecvError::Db
}
