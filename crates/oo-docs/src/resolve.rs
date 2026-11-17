//! Entity resolution, aliases, merges (10 §5.4). Board WS-BE3.
//!
//! L2 covers the pure core: canonical names and keys ([`normalize`], 10 §3.6)
//! and the exact-name resolver ([`exact`], 10 §5.4 steps 1, 3 to 5 without
//! context scoring). Fuzzy matching, identity attributes, merges and splits
//! belong to later cards.

pub mod exact;
pub mod normalize;
