//! Independent SplitMix64 and a tiny bootstrap.
//!
//! This file does not call the library PRNG while it builds the expected report.

use oo_trial_statistics::{evaluate, splitmix64_step, unbiased_index};

const SEED_0: [u64; 4] = [
    0xe220_a839_7b1d_cdaf,
    0x6e78_9e6a_a1b9_65f4,
    0x06c4_5d18_8009_454f,
    0xf88b_b8a8_724c_81ec,
];

fn independent_step(state: u64) -> (u64, u64) {
    let state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31), state)
}

fn independent_index(draw: u64, n: u64) -> Option<u64> {
    if n == 0 {
        return None;
    }
    let zone = (u64::MAX / n) * n;
    if draw < zone { Some(draw % n) } else { None }
}

fn independent_next(state: &mut u64, n: u64) -> usize {
    loop {
        let (draw, next) = independent_step(*state);
        *state = next;
        if let Some(index) = independent_index(draw, n) {
            return usize::try_from(index).expect("index");
        }
    }
}

fn reduce(numerator: i64, denominator: u64) -> (i64, u64) {
    if numerator == 0 {
        return (0, 1);
    }
    let mut a = numerator.unsigned_abs();
    let mut b = denominator;
    while b != 0 {
        let next = a % b;
        a = b;
        b = next;
    }
    (numerator / i64::try_from(a).expect("gcd"), denominator / a)
}

fn rat(numerator: i64, denominator: u64) -> String {
    let (numerator, denominator) = reduce(numerator, denominator);
    format!(r#"{{"denominator":{denominator},"numerator":{numerator}}}"#)
}

#[test]
fn known_splitmix_vector_locks_the_library() {
    let mut state = 0u64;
    let mut library = 0u64;
    for word in SEED_0 {
        let (output, next) = independent_step(state);
        let (library_output, library_next) = splitmix64_step(library);
        assert_eq!(output, word);
        assert_eq!(library_output, word);
        assert_eq!(next, library_next);
        state = next;
        library = library_next;
    }
    let (first, _) = independent_step(1);
    let (library_first, _) = splitmix64_step(1);
    assert_eq!(first, 0x910a_2dec_8902_5cc1);
    assert_eq!(library_first, first);
}

#[test]
fn rejection_tail_matches_the_independent_rule() {
    for draw in [0, 1, 5, u64::MAX - 2, u64::MAX - 1, u64::MAX] {
        for n in [1u64, 2, 3, 256] {
            assert_eq!(unbiased_index(draw, n), independent_index(draw, n));
        }
    }
    assert_eq!(independent_index(u64::MAX, 2), None);
    assert_eq!(independent_index(u64::MAX - 1, 2), None);
    assert_eq!(independent_index(u64::MAX, 3), None);
}

#[test]
fn eighty_resamples_match_an_independent_enumeration() {
    let low_rank: usize = 80 * 25 / 1000;
    let high_rank: usize = 80 * 975 / 1000;
    assert_eq!(low_rank, 2);
    assert_eq!(high_rank, 78);
    let bits_left = [true, false];
    let bits_right = [false, true];
    let mut state = 1u64;
    let mut diffs = Vec::with_capacity(80);
    for _ in 0..80 {
        let mut left = 0i64;
        let mut right = 0i64;
        for _ in 0..2 {
            let index = independent_next(&mut state, 2);
            if bits_left[index] {
                left += 1;
            }
            if bits_right[index] {
                right += 1;
            }
        }
        diffs.push(reduce(right - left, 2));
    }
    diffs.sort_by(|left, right| {
        let cross_left = i128::from(left.0) * i128::from(right.1);
        let cross_right = i128::from(right.0) * i128::from(left.1);
        cross_left.cmp(&cross_right)
    });
    let input = r#"{"algorithm":"oo.benchmark.paired-question-bootstrap/v1","resamples":80,"seed":"1","variants":[{"id":"left","questions":[{"id":"q0","trials":[{"index":0,"status":"pass"},{"index":1,"status":"pass"},{"index":2,"status":"pass"}]},{"id":"q1","trials":[{"index":0,"status":"pass"},{"index":1,"status":"fail"},{"index":2,"status":"pass"}]}],"tuple":{"as_of":"2026-10-01","budget":"tokens-8000","dataset":"public-slice","harness":"harness-0","model":"model-fixed","project":"logistics","role":"analyst","scorer_version":"scorer-1","snapshot":"snap-2026-10-01","suite":"paired-k3"}},{"id":"right","questions":[{"id":"q0","trials":[{"index":0,"status":"fail"},{"index":1,"status":"fail"},{"index":2,"status":"fail"}]},{"id":"q1","trials":[{"index":0,"status":"pass"},{"index":1,"status":"pass"},{"index":2,"status":"pass"}]}],"tuple":{"as_of":"2026-10-01","budget":"tokens-8000","dataset":"public-slice","harness":"harness-0","model":"model-fixed","project":"logistics","role":"analyst","scorer_version":"scorer-1","snapshot":"snap-2026-10-01","suite":"paired-k3"}}],"version":"oo.benchmark.trial-statistics/v1"}"#;
    let (low_n, low_d) = diffs[low_rank];
    let (high_n, high_d) = diffs[high_rank];
    let expect = format!(
        concat!(
            r#"{{"algorithm":"oo.benchmark.paired-question-bootstrap/v1","#,
            r#""gate":"passed","high_rank":{high},"low_rank":{low},"#,
            r#""percentile_high":{high_rat},"percentile_low":{low_rat},"#,
            r#""point_difference":{point},"question_count":2,"reannotation_count":0,"#,
            r#""resamples":80,"seed":"1","trial_count":3,"variants":["#,
            r#"{{"id":"left","pass_k":{pass},"pass_k_count":1}},"#,
            r#"{{"id":"right","pass_k":{pass},"pass_k_count":1}}],"#,
            r#""version":"oo.benchmark.trial-statistics-report/v1"}}"#
        ),
        high = high_rank,
        low = low_rank,
        high_rat = rat(high_n, high_d),
        low_rat = rat(low_n, low_d),
        point = rat(0, 2),
        pass = rat(1, 2),
    );
    let evaluation = evaluate(input).expect("document");
    assert!(evaluation.gate_passed);
    assert_eq!(evaluation.report, expect);
}
