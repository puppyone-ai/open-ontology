//! Kani proofs for KP5, KP6 and KP10 (80 section 3.2): the D26 decision, the
//! BRIEF ordering and degradation, the token estimate and the AIGC label.
//! `kernels.rs` holds the code the compiler calls; the harnesses here are
//! small bounded instances of the statements, all inputs symbolic.
//!
//! ```text
//! cargo kani -p oo-docs --harness kp5      # visibility
//! cargo kani -p oo-docs --harness kp6      # order, token estimate, degradation
//! cargo kani -p oo-docs --harness kp10     # AIGC label
//! ```

use std::cmp::Ordering;

use super::kernels::*;
use super::render::{AIGC_LABEL_LINE, CONTENT_PRODUCER};

// ------------------------------------------------------------------ KP5

const F: usize = 3;
const A: usize = 2;
const E: usize = 3;

/// D26 (FM8 on the Rust function): the output is a subset of the input, every
/// visible fact and alias keeps at least one evidence in scope and has known
/// ends, nothing is visible that the filter did not let through, and an entity
/// is visible exactly when a visible fact or alias points at it.
#[kani::proof]
#[kani::unwind(8)]
fn kp5_visible_decide_is_d26() {
    let fact_has: [bool; F] = kani::any();
    let alias_has: [bool; A] = kani::any();
    let known: [bool; E] = kani::any();
    let fact_subject: [usize; F] = kani::any();
    let fact_object: [Option<usize>; F] = kani::any();
    let alias_owner: [usize; A] = kani::any();
    // an index may point past the slots: an entity the reader did not know
    for i in 0..F {
        kani::assume(fact_subject[i] <= E);
        if let Some(o) = fact_object[i] {
            kani::assume(o <= E);
        }
    }
    for i in 0..A {
        kani::assume(alias_owner[i] <= E);
    }
    let (mut fv, mut av, mut ev) = ([true; F], [true; A], [true; E]);
    visible_decide(
        &fact_has,
        &fact_subject,
        &fact_object,
        &alias_has,
        &alias_owner,
        &known,
        &mut fv,
        &mut av,
        &mut ev,
    );
    let is_known = |e: usize| e < E && known[e];
    for i in 0..F {
        let ends_ok = is_known(fact_subject[i]) && fact_object[i].is_none_or(is_known);
        // exactly the facts with evidence in scope and known ends
        assert_eq!(fv[i], fact_has[i] && ends_ok);
        // subset: no visible fact without evidence in scope
        if fv[i] {
            assert!(fact_has[i]);
        }
    }
    for i in 0..A {
        assert_eq!(av[i], alias_has[i] && is_known(alias_owner[i]));
    }
    for e in 0..E {
        let by_fact = (0..F).any(|i| fv[i] && (fact_subject[i] == e || fact_object[i] == Some(e)));
        let by_alias = (0..A).any(|i| av[i] && alias_owner[i] == e);
        // an entity is visible iff something visible points at it (and it is a real entity)
        assert_eq!(ev[e], known[e] && (by_fact || by_alias));
    }
}

/// Out-of-scope content cannot make anything visible: a fact whose evidence is
/// all outside the scope, and an alias likewise, are dropped, and an entity only
/// they point at is dropped with them.
#[kani::proof]
#[kani::unwind(8)]
fn kp5_out_of_scope_evidence_shows_nothing() {
    let known: [bool; E] = [true; E];
    let fact_subject: [usize; F] = kani::any();
    let fact_object: [Option<usize>; F] = kani::any();
    let alias_owner: [usize; A] = kani::any();
    for i in 0..F {
        kani::assume(fact_subject[i] < E);
        if let Some(o) = fact_object[i] {
            kani::assume(o < E);
        }
    }
    for i in 0..A {
        kani::assume(alias_owner[i] < E);
    }
    // no evidence is in scope anywhere
    let (mut fv, mut av, mut ev) = ([true; F], [true; A], [true; E]);
    visible_decide(
        &[false; F],
        &fact_subject,
        &fact_object,
        &[false; A],
        &alias_owner,
        &known,
        &mut fv,
        &mut av,
        &mut ev,
    );
    assert!(fv.iter().all(|v| !v) && av.iter().all(|v| !v) && ev.iter().all(|v| !v));
}

// ------------------------------------------------------------------ KP6

fn ascii2() -> [u8; 2] {
    let b: [u8; 2] = kani::any();
    kani::assume(b[0] < 0x80 && b[1] < 0x80);
    b
}

fn key<'a>(
    f: bool,
    evs: usize,
    vf: &'a Option<[u8; 2]>,
    subj: &'a [u8; 2],
    fk: &'a Option<[u8; 2]>,
) -> RankKey<'a> {
    RankKey {
        seed_hit: f,
        valid_from: vf.as_ref().map(|b| std::str::from_utf8(b).unwrap()),
        evs,
        subject: std::str::from_utf8(subj).unwrap(),
        fact_key: fk.as_ref().map(|b| std::str::from_utf8(b).unwrap()),
    }
}

fn opt2() -> Option<[u8; 2]> {
    if kani::any() { Some(ascii2()) } else { None }
}

/// The ranking is a total preorder: any two keys compare, the comparison is
/// antisymmetric and transitive, and two keys are equal exactly when every
/// field is equal (so entries with different fact keys never tie, and the
/// order of a sorted list does not depend on where it started).
#[kani::proof]
#[kani::unwind(8)]
fn kp6_rank_order_is_total() {
    let (fa, fb, fc): (bool, bool, bool) = (kani::any(), kani::any(), kani::any());
    let (ea, eb, ec): (usize, usize, usize) = (kani::any(), kani::any(), kani::any());
    kani::assume(ea < 4 && eb < 4 && ec < 4);
    let (va, vb, vc) = (opt2(), opt2(), opt2());
    let (sa, sb, sc) = (ascii2(), ascii2(), ascii2());
    let (ka, kb, kc) = (opt2(), opt2(), opt2());
    let a = key(fa, ea, &va, &sa, &ka);
    let b = key(fb, eb, &vb, &sb, &kb);
    let c = key(fc, ec, &vc, &sc, &kc);
    // total and antisymmetric
    assert_eq!(rank_cmp_keys(&a, &b), rank_cmp_keys(&b, &a).reverse());
    assert_eq!(rank_cmp_keys(&a, &a), Ordering::Equal);
    // transitive
    if rank_cmp_keys(&a, &b) != Ordering::Greater && rank_cmp_keys(&b, &c) != Ordering::Greater {
        assert!(rank_cmp_keys(&a, &c) != Ordering::Greater);
    }
    // a tie only between identical keys
    if rank_cmp_keys(&a, &b) == Ordering::Equal {
        assert!(fa == fb && ea == eb && va == vb && sa == sb && ka == kb);
    }
}

/// `oo.token-estimate/v2` on concatenation (10 7.5), on the class streams the estimate reads: never
/// more than the two parts together, and joining two texts loses at most one token (two runs of
/// letters can merge into one). The character to class step is `char_class`; a concatenated text
/// has the concatenated class stream, so this covers every pair of texts.
#[kani::proof]
#[kani::unwind(12)]
fn kp6_token_estimate_is_subadditive() {
    use oo_builder::{CharClass, tokens_of_classes};
    let a: [CharClass; 4] = std::array::from_fn(|_| match kani::any::<u8>() % 4 {
        0 => CharClass::Cjk,
        1 => CharClass::Alnum,
        2 => CharClass::Space,
        _ => CharClass::Other,
    });
    let b: [CharClass; 4] = std::array::from_fn(|_| match kani::any::<u8>() % 4 {
        0 => CharClass::Cjk,
        1 => CharClass::Alnum,
        2 => CharClass::Space,
        _ => CharClass::Other,
    });
    let (na, nb): (usize, usize) = (kani::any(), kani::any());
    kani::assume(na <= 4 && nb <= 4);
    let ta = tokens_of_classes(a[..na].iter().copied());
    let tb = tokens_of_classes(b[..nb].iter().copied());
    let tj = tokens_of_classes(a[..na].iter().chain(b[..nb].iter()).copied());
    assert!(tj <= ta + tb);
    assert!(tj + 1 >= ta + tb);
    assert_eq!(tokens_of_classes(std::iter::empty()), 0);
}

/// `next_level` never goes back up, stays inside 0..=3 and only the last level is a fixed point.
#[kani::proof]
#[kani::unwind(2)]
fn kp6_next_level_is_monotone() {
    let l: u8 = kani::any();
    kani::assume(l <= REMOVED);
    assert!(next_level(l) >= l && next_level(l) <= REMOVED);
    assert_eq!(next_level(l) == l, l == REMOVED);
}

const N: usize = 4;

fn potential(levels: &[u8; N]) -> usize {
    levels.iter().map(|l| usize::from(REMOVED - l)).sum()
}

/// One degradation step (D25) on any section: the entry picked is in the section, not yet
/// removed, and the last such one (the lowest ranked); lowering it loses exactly one level and
/// no other level moves. The potential "levels left to lose" is at most 3 per entry and falls by
/// exactly 1 per step, so the loop of `build` ends after at most 3 steps per entry.
#[kani::proof]
#[kani::unwind(6)]
fn kp6_degradation_step_loses_exactly_one_level() {
    let secs: [u8; N] = kani::any();
    let mut levels: [u8; N] = kani::any();
    for i in 0..N {
        kani::assume(secs[i] < 5 && levels[i] <= REMOVED);
    }
    let sec: u8 = kani::any();
    kani::assume(sec < 5);
    let before = levels;
    assert!(potential(&before) <= 3 * N);
    match lowest_degradable_by(N, |i| (secs[i], levels[i]), sec) {
        Some(i) => {
            assert!(i < N && secs[i] == sec && levels[i] != REMOVED);
            for j in i + 1..N {
                assert!(!(secs[j] == sec && levels[j] != REMOVED));
            }
            levels[i] = next_level(levels[i]);
            for j in 0..N {
                assert!(levels[j] >= before[j]);
                assert!(j == i || levels[j] == before[j]);
            }
            assert_eq!(potential(&levels) + 1, potential(&before));
        }
        None => {
            assert!((0..N).all(|j| secs[j] != sec || levels[j] == REMOVED));
        }
    }
}

/// When no section in the degradation order has anything left to lower, every entry is removed:
/// the loop of `build` stops only at the floor (then it asks for a bigger budget).
#[kani::proof]
#[kani::unwind(6)]
fn kp6_degradation_stops_only_at_the_floor() {
    let secs: [u8; N] = kani::any();
    let levels: [u8; N] = kani::any();
    for i in 0..N {
        kani::assume(secs[i] < 5 && levels[i] <= REMOVED);
    }
    let stuck = DEGRADE_ORDER
        .iter()
        .all(|sec| lowest_degradable_by(N, |i| (secs[i], levels[i]), *sec).is_none());
    if stuck {
        assert!((0..N).all(|j| levels[j] == REMOVED));
    }
}

// ----------------------------------------------------------------- KP10

struct Buf {
    b: [u8; 420],
    n: usize,
}

impl Buf {
    fn new() -> Self {
        Self { b: [0; 420], n: 0 }
    }
    fn bytes(&self) -> &[u8] {
        &self.b[..self.n]
    }
}

impl Sink for Buf {
    fn put(&mut self, bytes: &[u8]) {
        for x in bytes {
            self.b[self.n] = *x;
            self.n += 1;
        }
    }
    fn pop(&mut self) {
        self.n -= 1;
    }
    fn tail_is_blank_line(&self) -> bool {
        self.n >= 2 && self.b[self.n - 1] == b'\n' && self.b[self.n - 2] == b'\n'
    }
}

/// `N` printable bytes, no line break: what a rendered BRIEF line or a pack id is made of.
fn ascii<const N: usize>() -> [u8; N] {
    let b: [u8; N] = kani::any();
    for x in &b {
        kani::assume(*x >= 0x20 && *x < 0x7f);
    }
    b
}

/// A generated Markdown file (D66): the label is the first line and exactly the
/// label; the front matter is the AIGC block, a function of pack id and path
/// only (no clock, no counter: the same inputs give the same bytes); and what
/// follows is the body, byte for byte, with only the trailing blank lines cut
/// to the single final newline: the label is a prefix, nothing else changes.
/// The lengths are fixed per harness (the bytes are symbolic): the write position
/// then stays concrete, which is what keeps the proof small.
fn md_file_statements<const NB: usize, const NP: usize, const N1: usize, const N2: usize>() {
    let basis: [u8; NB] = ascii();
    let path: [u8; NP] = ascii();
    let (l1, l2): ([u8; N1], [u8; N2]) = (ascii(), ascii());
    let body: [&[u8]; 2] = [&l1, &l2];

    let mut out = Buf::new();
    put_md_file(&mut out, &basis, &path, body.iter().copied());
    let got = out.bytes();

    // line 1 is the label and nothing else
    assert!(got.starts_with(AIGC_LABEL_LINE.as_bytes()));
    assert_eq!(got[AIGC_LABEL_LINE.len()], b'\n');
    // the front matter right after it, spelled out independently of the kernel
    let mut want = Buf::new();
    want.put(AIGC_LABEL_LINE.as_bytes());
    want.put(b"\n\n---\nAIGC:\n  Label: \"1\"\n  ContentProducer: \"");
    want.put(CONTENT_PRODUCER.as_bytes());
    want.put(b"\"\n  ProduceID: \"");
    want.put(&basis);
    want.put(b"/");
    want.put(&path);
    want.put(b"\"\n  ReservedCode1: \"");
    want.put(&basis);
    want.put(b"\"\n  ContentPropagator: \"\"\n  PropagateID: \"\"\n  ReservedCode2: \"\"\n---\n\n");
    let prefix = want.bytes();
    assert!(got.starts_with(prefix));

    // the body, with only the trailing blank lines normalised
    let mut tail = Buf::new();
    tail.put(&l1);
    tail.put(b"\n");
    tail.put(&l2);
    tail.put(b"\n");
    while tail.tail_is_blank_line() {
        tail.pop();
    }
    assert_eq!(&got[prefix.len()..], tail.bytes());
    // no clock reading: the only digits of the prefix are those of the pack id and path (the id twice) and
    // the three the format itself has (`Label: "1"`, `ReservedCode1`, `ReservedCode2`)
    let digits = prefix.iter().filter(|c| c.is_ascii_digit()).count();
    let inputs = basis.iter().filter(|c| c.is_ascii_digit()).count() * 2
        + path.iter().filter(|c| c.is_ascii_digit()).count()
        + 3;
    assert_eq!(digits, inputs);
}

macro_rules! kp10 {
    ($name:ident, $nb:literal, $np:literal, $n1:literal, $n2:literal) => {
        #[kani::proof]
        #[kani::unwind(340)]
        fn $name() {
            md_file_statements::<$nb, $np, $n1, $n2>();
        }
    };
}

// Lengths of pack id, path and the two body lines (one to two bytes each, symbolic content).
kp10!(
    kp10_label_first_front_matter_exact_body_untouched_1111,
    1,
    1,
    1,
    1
);
kp10!(
    kp10_label_first_front_matter_exact_body_untouched_2212,
    2,
    2,
    1,
    2
);
kp10!(
    kp10_label_first_front_matter_exact_body_untouched_1221,
    1,
    2,
    2,
    1
);

/// A body that ends in blank lines is cut to one final newline, and the label and front matter in
/// front of it do not change (empty lines are literals here: the write position stays concrete).
#[kani::proof]
#[kani::unwind(340)]
fn kp10_blank_lines_at_the_end_collapse() {
    let basis: [u8; 1] = ascii();
    let path: [u8; 1] = ascii();
    let line: [u8; 1] = ascii();
    let mut with_blanks = Buf::new();
    let body: [&[u8]; 3] = [&line, b"", b""];
    put_md_file(&mut with_blanks, &basis, &path, body.iter().copied());
    let mut plain = Buf::new();
    put_md_file(&mut plain, &basis, &path, std::iter::once(&line[..]));
    // trailing blank lines change nothing: the bytes equal the file without them
    assert_eq!(with_blanks.bytes(), plain.bytes());
    assert_eq!(plain.bytes()[plain.bytes().len() - 1], b'\n');
    assert_ne!(plain.bytes()[plain.bytes().len() - 2], b'\n');
}

/// The prefix is a pure function of pack id and path: two buffers give the same bytes and a
/// different body starts with the same bytes.
#[kani::proof]
#[kani::unwind(340)]
fn kp10_prefix_is_deterministic() {
    let basis: [u8; 2] = ascii();
    let path: [u8; 2] = ascii();
    let (mut x, mut y) = (Buf::new(), Buf::new());
    put_md_prefix(&mut x, &basis, &path);
    put_md_prefix(&mut y, &basis, &path);
    assert_eq!(x.bytes(), y.bytes());
    let line: [u8; 1] = ascii();
    let mut z = Buf::new();
    put_md_file(&mut z, &basis, &path, std::iter::once(&line[..]));
    assert!(z.bytes().starts_with(x.bytes()));
}
