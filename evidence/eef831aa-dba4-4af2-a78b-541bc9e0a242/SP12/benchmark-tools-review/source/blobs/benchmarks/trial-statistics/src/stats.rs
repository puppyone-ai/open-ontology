//! SplitMix64 and the paired question bootstrap from `CONTRACT.md`.

use serde::Serialize;

use crate::parse::{Parsed, Status};

/// One SplitMix64 step. `state` is the state before the step.
///
/// The result is `(output, next_state)`.
pub fn splitmix64_step(state: u64) -> (u64, u64) {
    let state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    let output = z ^ (z >> 31);
    (output, state)
}

/// Unbiased residue of `draw` in `0..n`.
///
/// `None` means the draw falls in the rejected tail and must be replaced.
/// `n == 0` is `None`.
pub fn unbiased_index(draw: u64, n: u64) -> Option<u64> {
    if n == 0 {
        return None;
    }
    let zone = (u64::MAX / n) * n;
    if draw < zone { Some(draw % n) } else { None }
}

fn next_index(state: &mut u64, n: u64) -> usize {
    loop {
        let (draw, next) = splitmix64_step(*state);
        *state = next;
        if let Some(index) = unbiased_index(draw, n) {
            return usize::try_from(index).expect("index fits");
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct Rational {
    denominator: u64,
    numerator: i64,
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let next = a % b;
        a = b;
        b = next;
    }
    a
}

fn reduce(numerator: i64, denominator: u64) -> Rational {
    if numerator == 0 {
        return Rational {
            denominator: 1,
            numerator: 0,
        };
    }
    let g = gcd(numerator.unsigned_abs(), denominator);
    Rational {
        denominator: denominator / g,
        numerator: numerator / i64::try_from(g).expect("gcd fits"),
    }
}

fn cmp_rational(left: &Rational, right: &Rational) -> std::cmp::Ordering {
    let cross_left = i128::from(left.numerator) * i128::from(right.denominator);
    let cross_right = i128::from(right.numerator) * i128::from(left.denominator);
    cross_left.cmp(&cross_right)
}

fn passes(trials: &[Status]) -> bool {
    trials.iter().all(|status| *status == Status::Pass)
}

fn needs_reannotation(trials: &[Status]) -> bool {
    trials.contains(&Status::NeedsReannotation)
}

#[derive(Serialize)]
struct VariantReport {
    id: String,
    pass_k: Rational,
    pass_k_count: u32,
}

#[derive(Serialize)]
struct Report {
    algorithm: &'static str,
    gate: &'static str,
    high_rank: u32,
    low_rank: u32,
    percentile_high: Rational,
    percentile_low: Rational,
    point_difference: Rational,
    question_count: u32,
    reannotation_count: u32,
    resamples: u32,
    seed: String,
    trial_count: u32,
    variants: Vec<VariantReport>,
    version: &'static str,
}

pub(crate) struct Rendered {
    pub gate_passed: bool,
    pub report: String,
}

pub(crate) fn render(parsed: &Parsed) -> Rendered {
    let n = parsed.variants[0].questions.len();
    let n_u = u64::try_from(n).expect("question count");
    let mut bits = [Vec::new(), Vec::new()];
    let mut counts = [0u32, 0u32];
    let mut reannotation_count = 0u32;
    for (left, right) in parsed.variants[0]
        .questions
        .iter()
        .zip(parsed.variants[1].questions.iter())
    {
        let left_pass = passes(&left.trials);
        let right_pass = passes(&right.trials);
        bits[0].push(left_pass);
        bits[1].push(right_pass);
        if left_pass {
            counts[0] += 1;
        }
        if right_pass {
            counts[1] += 1;
        }
        if needs_reannotation(&left.trials) || needs_reannotation(&right.trials) {
            reannotation_count += 1;
        }
    }
    let mut state = parsed.seed;
    let mut diffs = Vec::with_capacity(usize::try_from(parsed.resamples).expect("resamples"));
    for _ in 0..parsed.resamples {
        let mut drawn = [0i64, 0i64];
        for _ in 0..n {
            let index = next_index(&mut state, n_u);
            for variant in 0..2 {
                if bits[variant][index] {
                    drawn[variant] += 1;
                }
            }
        }
        diffs.push(reduce(drawn[1] - drawn[0], n_u));
    }
    diffs.sort_by(cmp_rational);
    let low_rank = parsed.resamples * 25 / 1000;
    let high_rank = parsed.resamples * 975 / 1000;
    let gate_passed = reannotation_count == 0;
    let report = Report {
        algorithm: "oo.benchmark.paired-question-bootstrap/v1",
        gate: if gate_passed { "passed" } else { "failed" },
        high_rank,
        low_rank,
        percentile_high: diffs[usize::try_from(high_rank).expect("rank")],
        percentile_low: diffs[usize::try_from(low_rank).expect("rank")],
        point_difference: reduce(i64::from(counts[1]) - i64::from(counts[0]), n_u),
        question_count: u32::try_from(n).expect("question count"),
        reannotation_count,
        resamples: parsed.resamples,
        seed: parsed.seed_text.clone(),
        trial_count: parsed.trial_count,
        variants: parsed
            .variants
            .iter()
            .enumerate()
            .map(|(index, variant)| VariantReport {
                id: variant.id.clone(),
                pass_k: reduce(i64::from(counts[index]), n_u),
                pass_k_count: counts[index],
            })
            .collect(),
        version: "oo.benchmark.trial-statistics-report/v1",
    };
    Rendered {
        gate_passed,
        report: serde_json::to_string(&report).expect("report json"),
    }
}
