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

/// Base `prompt_version` (no promoted types).
pub const PROMPT_VERSION: &str = "extract/1";

/// An open type a person promoted (`type_promote` overlay, B-S9-02): `Thing` instances with this `label` are
/// extracted under the confirmed `type_name`, and the label is constrained in the prompt.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConstrainedType {
    pub label: String,
    pub type_name: String,
}

/// `prompt_version` for a set of promoted types: the base version while there are none, else
/// `extract/1+t<8 hex>` of the sorted set, so promoting (or withdrawing) a type changes the version, the cache
/// keys and which files are `outdated`.
pub fn prompt_version(types: &[ConstrainedType]) -> String {
    if types.is_empty() {
        return PROMPT_VERSION.to_owned();
    }
    let mut sorted: Vec<&ConstrainedType> = types.iter().collect();
    sorted.sort();
    let mut h = blake3::Hasher::new();
    h.update(b"oo.docs.llm-types/v1\0");
    for t in sorted {
        h.update(t.label.as_bytes());
        h.update(b"\0");
        h.update(t.type_name.as_bytes());
        h.update(b"\0");
    }
    format!("{PROMPT_VERSION}+t{}", &h.finalize().to_hex()[..8])
}
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
