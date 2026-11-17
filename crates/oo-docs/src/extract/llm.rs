//! LLM supplement (10 §5.2, D18, D19). Board WS-BE3.
//!
//! Rules run first and are always enough to use the product; the model only
//! adds candidates. Everything the model says goes to drafts with
//! `origin = 'llm'` and must carry a block id and a verbatim quote:
//!
//! * [`batch`]: sections of the IR become batches of at most 3,000 tokens;
//! * [`prompt`]: `extract/1` prompt and the block level cache key;
//! * [`schema`]: `oo.docs.llm-extract/v1` validation;
//! * [`post`]: quote check, domain and range, `Thing` labels, mapping to an
//!   [`Extraction`](crate::extract::model::Extraction) (confidence 8500, 7000,
//!   5000 for high, medium, low; `supersedes` is queued by the review queue,
//!   never published by itself);
//! * [`usage`]: call reservation, the per file and per Drive daily caps
//!   and the usage rows behind `GET /usage`;
//! * [`egress`]: the loopback `model_egress.py` client and a replay port that
//!   serves recorded responses (no network in tests);
//! * [`run`]: the loop over batches (cache, reserve, call, validate, retry
//!   once, store, map). A reached cap stops the model for that file or Drive
//!   and keeps what was extracted; the rules' result is never affected.
//!
//! The model endpoint and key are referenced by path only
//! (`~/.config/open-ontology/secrets/...`, handed to the egress process by
//! `--config`); nothing here reads or prints them.

pub mod batch;
pub mod egress;
pub mod post;
pub mod prompt;
pub mod run;
pub mod schema;
pub mod usage;

/// `prompt_version` stored with every LLM fact.
pub const PROMPT_VERSION: &str = "extract/1";
/// Output schema tag.
pub const SCHEMA_TAG: &str = "oo.docs.llm-extract/v1";
/// Largest batch, in `cjk_calibrated_tokens`.
pub const MAX_BATCH_TOKENS: u64 = 3_000;
/// Target size of a section when the file has no headings.
pub const SECTION_TOKENS: u64 = 1_500;
/// Request parameters (5.2.2): temperature 0, 2048 tokens, no streaming.
pub const MAX_TOKENS: u32 = 2_048;
/// Default model calls per file version (D19).
pub const FILE_CALL_LIMIT: u32 = 20;
