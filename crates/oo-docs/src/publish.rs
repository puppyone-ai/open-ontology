//! Single-writer publish and gates (10 §6). Board WS-BE5 owns the module, the
//! L2 publisher (B-S2-07) implements DOC-G1 and DOC-G2 and the version row.
//!
//! * [`gates`]: pure gate checks (schema, evidence);
//! * [`write`]: store one parsed file's rule output in `_oo_docs` as drafts;
//! * [`run`]: the publisher transaction that turns eligible drafts into
//!   ontology version `vN` with `gate_report` rows.

pub mod gates;
pub mod run;
pub mod write;

#[cfg(test)]
mod tests;
