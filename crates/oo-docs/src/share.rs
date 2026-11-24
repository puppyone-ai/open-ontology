//! Share lifecycle, refs credentials, drift, `UPDATES.md` stream, `check`,
//! task groups and coverage (10 §7.6, §7.8, §16). Board lane C, S6.
//!
//! The engine writes nothing outside its state directory: the physical mount
//! belongs to the `oo` CLI and the `oo serve` daemon. Everything here is
//! database state plus the pack bytes in the CAS.

pub mod changes;
pub mod check;
pub mod coverage;
pub mod credential;
pub mod drift;
pub mod group;
pub mod lifecycle;
pub mod updates;
