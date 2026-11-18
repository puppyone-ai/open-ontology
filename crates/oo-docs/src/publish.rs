//! Single-writer publish and gates (10 §6). Board WS-BE5 owns the module, the
//! L2 publisher (B-S2-07) implements DOC-G1 and DOC-G2 and the version row.
//!
//! * [`gates`]: pure gate checks (schema, evidence); DOC-G3 to G5 run in `run`;
//! * [`debounce`]: the 2 second publish window;
//! * [`write`]: store one parsed file's rule output in `_oo_docs` as drafts;
//! * [`run`]: the publisher transaction that turns eligible drafts into
//!   ontology version `vN` with `gate_report` rows.

pub mod constraints;
pub mod debounce;
pub mod gates;
pub mod hashes;
pub mod projection;
pub mod reprocess;
pub mod run;
pub mod write;

#[cfg(test)]
mod tests;
