//! The stored class of a recording (D74 revision, 10 17.4): one row per blob in `_oo_docs.media_profile`.
//!
//! The rules decide first (`extract::rules::media_class`) and their verdict is written here whenever the
//! recording is extracted; a verdict of the model (asked only when the rules were unsure) is
//! kept and wins over the rules from then on, so a reprocess does not undo it.

use oo_core::docir::{DocFormat, DocIr};
use serde_json::Value;
use sqlx::{PgConnection, Row};

use crate::extract::llm::egress::LlmPort;
use crate::extract::llm::prompt::ChatRequest;
use crate::extract::rules::media_class::{self, MediaClass, MediaProfile};
use crate::store::StoreError;

/// A row of `media_profile`.
#[derive(Clone, Debug, PartialEq)]
pub struct Stored {
    pub class: MediaClass,
    pub kind: String,
    pub confidence_bp: i32,
    /// `rules` or `model`.
    pub source: String,
    pub signals: Value,
}

/// True for the formats that have speakers to tell apart.
pub fn is_media(ir: &DocIr) -> bool {
    matches!(ir.format, DocFormat::Audio | DocFormat::Video)
}

pub async fn load(conn: &mut PgConnection, b3: &str) -> Result<Option<Stored>, StoreError> {
    let row = sqlx::query(
        "SELECT class, kind, confidence_bp, source, signals::text AS signals FROM _oo_docs.media_profile WHERE b3 = $1",
    )
    .bind(b3)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(r) = row else { return Ok(None) };
    let class: String = r.try_get("class")?;
    Ok(MediaClass::parse(&class).map(|class| Stored {
        class,
        kind: r.try_get("kind").unwrap_or_default(),
        confidence_bp: r.try_get("confidence_bp").unwrap_or(0),
        source: r.try_get("source").unwrap_or_default(),
        signals: r
            .try_get::<String, _>("signals")
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null),
    }))
}

async fn store(
    conn: &mut PgConnection,
    b3: &str,
    class: MediaClass,
    kind: &str,
    confidence_bp: i32,
    source: &str,
    signals: &Value,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO _oo_docs.media_profile (b3, class, kind, confidence_bp, source, signals) \
         VALUES ($1, $2, $3, $4, $5, $6::jsonb) \
         ON CONFLICT (b3) DO UPDATE SET class = EXCLUDED.class, kind = EXCLUDED.kind, \
           confidence_bp = EXCLUDED.confidence_bp, source = EXCLUDED.source, signals = EXCLUDED.signals, \
           updated_at = clock_timestamp()",
    )
    .bind(b3)
    .bind(class.as_str())
    .bind(kind)
    .bind(confidence_bp)
    .bind(source)
    .bind(signals.to_string())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// The class extraction acts on: a stored verdict of the model or a person, else the rules' own, which is
/// written down. An undecided recording stays `unsure` here (and does not identify speakers).
pub async fn resolve(
    conn: &mut PgConnection,
    ir: &DocIr,
    path: &str,
) -> Result<MediaClass, StoreError> {
    if let Some(s) = load(conn, &ir.blob_b3).await?
        && s.source != "rules"
        && s.class != MediaClass::Unsure
    {
        return Ok(s.class);
    }
    let p: MediaProfile = media_class::classify(ir, path);
    store(
        conn,
        &ir.blob_b3,
        p.class,
        p.kind,
        i32::from(p.confidence_bp),
        "rules",
        &p.signals_json(),
    )
    .await?;
    Ok(p.class)
}

/// Ask the model about an undecided recording and keep a clear answer. `Ok(None)`: the model was not asked or
/// did not answer clearly (the recording stays undecided and is treated as not a conversation).
pub async fn ask_model(
    conn: &mut PgConnection,
    port: &dyn LlmPort,
    ir: &DocIr,
    title: &str,
) -> Result<Option<MediaClass>, StoreError> {
    let Some(s) = load(conn, &ir.blob_b3).await? else {
        return Ok(None);
    };
    if s.class != MediaClass::Unsure || s.source != "rules" {
        return Ok(None);
    }
    let (system, user) = media_class::prompt(ir, title);
    let req = ChatRequest {
        model: port.model().to_owned(),
        system,
        user,
    };
    let Ok(reply) = port.complete(&req).await else {
        return Ok(None);
    };
    let Some(class) = media_class::parse_answer(&reply.content) else {
        return Ok(None);
    };
    let kind = if class == MediaClass::Conversation {
        "conversation"
    } else {
        "monologue"
    };
    store(conn, &ir.blob_b3, class, kind, 7000, "model", &s.signals).await?;
    Ok(Some(class))
}
