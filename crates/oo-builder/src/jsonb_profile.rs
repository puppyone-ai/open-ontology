//! Full BSON document sealed onto one jsonb OID 3802 column.
//!
//! The catalog is compiled first. A bad catalog returns before the document
//! seal. The profile carries Arrow metadata, quality, and exact hashes. It
//! does not store the raw BSON bytes and does not open a database.

use serde_json::{Map, Value, json};

use oo_core::api::payload_sha256;
use oo_core::jsonb_doc::{self, SealedJsonbDocument};

use crate::canonical::canonical_json;
use crate::sealed_import::{SealedImportDraft, SealedImportError, compile_sealed_import_catalog};

/// Profile protocol. Task 41 and task 06 read this object, not a private dialect.
pub const JSONB_PROFILE_PROTOCOL: &str = jsonb_doc::PROFILE_PROTOCOL;

/// Compiler name recorded on the profile and appended to source-semantics builder calls.
pub const JSONB_PROFILE_COMPILER: &str = "compile_jsonb_document_profile";

/// Compiled document profile. `document` is canonical JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonbDocumentProfile {
    pub protocol: &'static str,
    pub compiler: &'static str,
    pub document: String,
    pub document_identity_hash: String,
    pub schema_digest: String,
    pub source_engine: String,
    pub dataset: String,
    pub column: String,
    pub raw_bson_sha256: String,
    pub pg_jsonb_sha256: String,
    pub normalized_representation: &'static str,
}

/// Compile one catalog, then seal one full document onto its jsonb column.
///
/// The identity preimage is `oo.builder.document-jsonb-profile/v1`, the seal
/// identity, the schema digest, and the source engine, each on its own line.
pub fn compile_jsonb_document_profile(
    catalog_json: &str,
    document_json: &str,
) -> Result<JsonbDocumentProfile, SealedImportError> {
    let draft = compile_sealed_import_catalog(catalog_json)?;
    let sealed = jsonb_doc::seal_bson_jsonb_document(document_json)
        .map_err(|err| SealedImportError::from_full(err.code, err.path, err.message))?;
    let column = draft
        .columns
        .iter()
        .find(|item| item.dataset == sealed.dataset && item.name == sealed.column)
        .ok_or_else(|| {
            SealedImportError::draft(
                "column.absent",
                "/document/column",
                "document column is not in the sealed catalog",
            )
        })?;
    if column.target_type != "jsonb"
        || column.target_oid != jsonb_doc::JSONB_OID
        || column.target_typmod != jsonb_doc::JSONB_TYPMOD
    {
        return Err(SealedImportError::draft(
            "type.ambiguous",
            "/document/column",
            "document column is not jsonb oid 3802 typmod -1",
        ));
    }
    if column.precision_loss {
        return Err(SealedImportError::draft(
            "precision_loss",
            "/document/column",
            "jsonb document column is marked precision loss",
        ));
    }
    if let (Some(sealed_source), Some(catalog_source)) = (&sealed.source_type, &column.source_type)
        && sealed_source != catalog_source
    {
        return Err(SealedImportError::draft(
            "source_type.rejected",
            "/document/source_type",
            "document source type does not match the sealed column",
        ));
    }
    Ok(finish(&draft, &sealed))
}

fn finish(draft: &SealedImportDraft, sealed: &SealedJsonbDocument) -> JsonbDocumentProfile {
    let document_identity_hash = payload_sha256(
        format!(
            "{JSONB_PROFILE_PROTOCOL}\n{}\n{}\n{}\n",
            sealed.identity_hash, draft.schema_digest, draft.source_engine
        )
        .as_bytes(),
    );
    let mut metadata = Map::new();
    for (key, value) in &sealed.metadata {
        metadata.insert(key.clone(), Value::String(value.clone()));
    }
    let body = json!({
        "arrow": {
            "metadata": Value::Object(metadata),
            "type": sealed.arrow_type,
        },
        "column": &sealed.column,
        "compiler": JSONB_PROFILE_COMPILER,
        "dataset": &sealed.dataset,
        "document_identity_hash": &document_identity_hash,
        "extended_json": &sealed.extended_json,
        "normalized_representation": draft.normalized_representation,
        "double": {
            "encoding": jsonb_doc::DOUBLE_ENCODING,
            "nan_payload": jsonb_doc::DOUBLE_NAN_PAYLOAD,
            "negative_zero": jsonb_doc::DOUBLE_NEGATIVE_ZERO,
            "non_finite": jsonb_doc::DOUBLE_NON_FINITE,
        },
        "oid": sealed.oid,
        "pg_jsonb_sha256": &sealed.pg_jsonb_sha256,
        "precision": {"float": jsonb_doc::FLOAT_PRECISION},
        "protocol": JSONB_PROFILE_PROTOCOL,
        "quality": {"loss_concealed": false},
        "raw_bson_sha256": &sealed.raw_bson_sha256,
        "schema_digest": &draft.schema_digest,
        "source_engine": &draft.source_engine,
        "source_type": &sealed.source_type,
        "typmod": sealed.typmod,
    });
    let document = canonical_json(&body);
    JsonbDocumentProfile {
        protocol: JSONB_PROFILE_PROTOCOL,
        compiler: JSONB_PROFILE_COMPILER,
        document,
        document_identity_hash,
        schema_digest: draft.schema_digest.clone(),
        source_engine: draft.source_engine.clone(),
        dataset: sealed.dataset.clone(),
        column: sealed.column.clone(),
        raw_bson_sha256: sealed.raw_bson_sha256.clone(),
        pg_jsonb_sha256: sealed.pg_jsonb_sha256.clone(),
        normalized_representation: draft.normalized_representation,
    }
}
