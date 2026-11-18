//! Deterministic rules and LLM supplement (10 §5). Board WS-BE3.
//!
//! L2 (B-S2-07) ships `rules/1` for md and txt: directory conventions,
//! frontmatter, heading structure, `key: value` lines and file-name dates.
//! The output ([`model::Extraction`]) is pure data: mentions, aliases and
//! fact candidates with evidence (block id + anchor). Persisting them is
//! `publish::write`; binding mentions to stored entities is `resolve`.
//! The LLM supplement (5.2) is a later card.

pub mod confidence;
pub mod links;
pub mod llm;
pub mod model;
pub mod rules;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_s5;
