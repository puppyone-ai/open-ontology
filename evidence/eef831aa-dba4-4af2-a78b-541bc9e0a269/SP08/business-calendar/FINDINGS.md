# Findings

Reviewer: this session, sole coder and reviewer. This is not a separate reviewer and it is not parent acceptance.

## Verdict

The frozen child matches `linux/impl-4` and the fresh-target `linux/review-1`. Both are Linux arm64 Rust 1.94 on fixed commit `0bcd494457091d4b55c82fc6b9922b5bc5dc44c4`. Focused tests exited 0: 8 library, 5 CLI, 8 hand-computed. Clippy `--all-targets -D warnings` exited 0 on both runs. The child lock's registry triples are a subset of the root lock (`registry=115 path=2 missing=0 mismatch=0`). The root lock hash did not change. The child lock hash did not change during review.

I read the impl-4 and review-1 phase exits, test logs, clippy exits, `lock-subset.txt`, and `lock-hash.txt` after each container exited.

## What the proof shows

The example calendar at `2026-04-01T03:30:00Z` in `Asia/Shanghai` resolves today and the fiscal-April anchors, and emits the four sample phrases in `BTreeMap` order. A fiscal month of 2 on 2026-03-15 yields quarter start 2026-02-01. Leap-day, year-boundary, and month-to-date `min(...)` cases match the hand literals. New York 2026-03-08 is 82800 seconds and 2026-11-01 is 90000 seconds. Sao Paulo 2018-11-04 midnight is nonexistent. Apia 2011-12-30 midnight is nonexistent, and the inclusive end of an exclusive end on 2011-12-31 remains the civil day 2011-12-30. Paris 1976-09-26 midnight stays ambiguous, with fold0 earlier than fold1. The CLI checks exact stdout bytes, help text, input bounds, and redaction of an invalid phrase label.

`oo_core` checks anchor expressions and does not evaluate them. This crate evaluates the same grammar. An expression the checker accepts and the evaluator cannot type is `expression_refused`. It is not approximated.

## Limits that stay in force

`finance_approval` and `authenticated` are false. The closure record hash is the caller's declared hex. It is not a finance approval and it is not a digest of the label.

`week_start` is stored and is not turned into a week anchor. A missing configured anchor is not replaced by a natural month or by `today`. There is no process clock.

A local midnight that is a gap or an overlap is not moved. `civil_day_seconds` is present only when both midnights are unique.

The helper does not create a PostgreSQL view, write `_oo.calendar_config`, or take a database connection.

## Retained failures

`linux/impl-1` did not compile. `linux/impl-2` failed one CLI assertion and two clippy lints. `linux/impl-3` passed clippy and failed the source scan on `std::process::ExitCode`. Those directories stay.

## Not accepted here

Parent acceptance, SEM-D01, a restricted-owner calendar view, an authenticated close, and a release are outside this child.
