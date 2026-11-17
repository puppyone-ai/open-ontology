//! Kani proofs for KP7 (80 section 3.2): the state transition function is
//! total, never panics and agrees with the 10 section 4.2 table for every
//! (state, target) pair. Run with `cargo kani -p oo-docs --harness kp7`.
//!
//! `spec_legal` is a second, independent statement of the table (explicit
//! pairs, not the `EDGES` slice `is_legal` walks). `conformance_state.rs`
//! checks the same table against the Quint model for the same pairs.

use super::{FileState, FileState::*, is_legal, is_side};

fn spec_legal(from: Option<FileState>, to: FileState) -> bool {
    let Some(from) = from else {
        return to == Stored;
    };
    match (from, to) {
        // lifecycle rows: any live state may be superseded or deleted;
        // superseded may still be deleted, deleted may not be superseded
        (Superseded | Deleted, Superseded) | (Deleted, Deleted) => false,
        (_, Superseded | Deleted) => true,
        // main path and its failures
        (Stored, TooLarge | Unsupported | Queued | Duplicate | ParseFailed) => true,
        (Queued, Unsupported | Encrypted | Parsing) => true,
        (Parsing, Parsed | Queued | ParseFailed) => true,
        (Parsed, Extracting) => true,
        (Extracting, Indexed | ExtractPartial) => true,
        (Indexed, Published | Outdated) => true,
        (ExtractPartial, Published | Queued | Outdated) => true,
        (Published, Outdated) => true,
        (ParseFailed, Queued | Outdated) => true,
        (Unsupported | Duplicate, Outdated) => true,
        (Outdated, Parsing) => true,
        // re-entry
        (Superseded | Deleted, Stored) => true,
        _ => false,
    }
}

fn from_index(i: usize) -> Option<FileState> {
    if i == FileState::ALL.len() {
        None
    } else {
        Some(FileState::ALL[i])
    }
}

/// Every (from, to), including "no row" as `from`: defined, no panic, equal to
/// the table.
#[kani::proof]
#[kani::unwind(20)]
fn kp7_transition_function_matches_the_table() {
    let f: usize = kani::any();
    let t: usize = kani::any();
    kani::assume(f <= FileState::ALL.len() && t < FileState::ALL.len());
    let (from, to) = (from_index(f), FileState::ALL[t]);
    assert_eq!(is_legal(from, to), spec_legal(from, to));
}

/// The table has no dead end: every state has a legal successor, except that
/// nothing leaves a state in the side set other than the lifecycle rows and
/// `outdated`.
#[kani::proof]
#[kani::unwind(20)]
fn kp7_every_state_has_a_way_out() {
    let f: usize = kani::any();
    kani::assume(f < FileState::ALL.len());
    let from = FileState::ALL[f];
    assert!(FileState::ALL.iter().any(|t| is_legal(Some(from), *t)));
    if is_side(from) && from != Unsupported && from != Duplicate {
        // too_large and encrypted: only the lifecycle rows leave them
        assert!(
            FileState::ALL
                .iter()
                .filter(|t| is_legal(Some(from), **t))
                .all(|t| matches!(t, Superseded | Deleted))
        );
    }
}

/// Database text and the enum round trip for every state, and creation is
/// only into `stored`.
#[kani::proof]
#[kani::unwind(20)]
fn kp7_state_names_round_trip() {
    let f: usize = kani::any();
    kani::assume(f < FileState::ALL.len());
    let s = FileState::ALL[f];
    assert_eq!(FileState::parse(s.as_str()), Some(s));
    assert!(is_legal(None, s) == (s == Stored));
}
