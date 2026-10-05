# HANDOFF a219 mapped shadow statistics

Product commit `613aa981dc2c57c0005cc6589e1a8ee7757f2a69`. Parent `5af5db7e9bc27a32bacb4d617bd2393c8edc2cb6`. Fixed archive `c2db9c430b822a824a1062b6c3c7345924df7dd8`.

## Read first

- `REPORT.md` for the green exits and the measured facts.
- `FINDINGS.md` F1 and F2 before changing the idle probe or the `Transaction` borrows.
- `contracts/pipeline/target-statistics-v1.md` for the refusal order and the simple-query disposition rule.
- `linux/overlay/live/facts.txt` and `linux/overlay/live/catalog-facts.txt`.

## What landed

`analyze_mapped_shadows` is an explicit operator maintenance call. `lib.rs` only exports the module. The live proof imported through the existing public import API, observed `reltuples` `-1`, refused stale, missing, runtime, and readonly callers, ran `ANALYZE`, observed finite bits `1073741824`, then loaded the existing policy catalog with `UnknownRows::Refuse` and counted 2 rows. Source audit delta was 0.

## Do not treat this as acceptance

M1-D06A has a green a219 proof in this directory. This handoff does not mark the plan card, M1-I01, M1-I02, or a release. The receipt is not publication. The `ANALYZE` value is not an exact row count. Import was not changed to call this function.

## Failed attempts

Keep `linux/overlay/attempt-1` through `attempt-6`. They record the borrow, compile, clippy, and idle-probe failures. The green consoles are `linux/overlay/console.log` and `linux/preoverlay/console.log`.

## Cleanup

`cleanup/run.log` removed container `oo-a219-pg`, the three `oo-a219-eef831aa-*` volumes, network `oo-a219-eef831aa-net`, and `/tmp/oo-a219-eef831aa-dba4-4af2-a78b-541bc9e0a219`. Pinned Rust and PostgreSQL images stayed. No prune. Other owners' containers were not removed.

## Next reader

A statement-timeout refusal (`57014`) is classified and was not provoked. A second live case would be a new run, not a reinterpretation of this one. The catalog proof file is evidence. It is not an `oo-gateway` change.
