//! Values written in words, normalised (10 §5.1.2, §5.3). Pure, deterministic, no model.
//!
//! A number, an amount, a date or a ratio is often written in words: `六百八十五万`, `6.85 million`, `March sixteenth`,
//! `三月十六号`, `下周二`, `百分之十二`, `12 percent`. The same value written differently must be the same value, so
//! that two files that state it link through the one open Amount, Date or Ratio entity (`extract::anchors`), and so
//! that a cell of a table slot gets the type of its property (`structure::tablefill`). This module reads those
//! spellings into canonical typed values:
//!
//! * [`Dec`]: exact decimal arithmetic (no floating point), so `3.5亿` is `350000000`;
//! * [`zh`] and [`en`]: whole-string number readers (Chinese numerals with `万/亿/千/百/十/两/零`, English number words
//!   and scales, mixed digits);
//! * [`money`]: amounts with a currency (`¥ $ € £ 元 美元 USD RMB`) and the scale written (`万`, `million`);
//! * [`ratio`]: percentages (`12%`, `百分之十二`, `twelve percent`, `三成`) and `a:b` ratios;
//! * [`date`]: absolute dates in Chinese and English, and relative dates (`下周二`, `next Tuesday`, `明天`)
//!   resolved against the date of the document or the meeting. A year that is missing is taken from that date only
//!   when it is unambiguous; otherwise the text is not a date here (kept as text, not linked);
//! * [`scan`]: the values inside running text, with their spans, for the anchor pass.
//!
//! Everything is conservative: a candidate that is an ordinal (`第一`), a degree adverb (`十分好`), a phone number, an
//! id or a count of things (`一万人`) is not a value.

pub mod date;
pub mod dec;
pub mod en;
pub mod money;
pub mod ratio;
pub mod scan;
pub mod zh;

#[cfg(test)]
mod tests;

pub use date::{DateCtx, How, NormDate, Ymd};
pub use dec::Dec;
pub use money::Money;
pub use ratio::Ratio;
pub use scan::{Found, Value};

/// Version of the normaliser; written into stage details so a change of rules is visible.
pub const VERSION: &str = "values/1";
