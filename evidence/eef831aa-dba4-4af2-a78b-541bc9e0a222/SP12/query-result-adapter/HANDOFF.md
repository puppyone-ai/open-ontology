# HANDOFF a222 query-result adapter

Product commit `ed661809cf6c1e8df37af5842838dd1eb839c98a`. Parent `c5f4019574541296e15628325738864d97e4a47f`. Fixed archive `c2db9c430b822a824a1062b6c3c7345924df7dd8`.

## Read first

- `REPORT.md` for the green exits and the saved stdout.
- `FINDINGS.md` F3 through F6 before rerunning `linux/proof/inside.sh` on a warm target volume.
- `benchmarks/query-result-adapter/CONTRACT.md` for the refusal tokens.
- `linux/proof/proof.stdout` and `linux/proof/live/oracle.txt`.

## What landed

`benchmarks/query-result-adapter` maps one caller header plus one `query-pg` JSONL result into a typed actual. The green proof compiled `query-pg` and `result-comparator` from the fixed archive, ran the existing live fixture against `oo-a222-pg`, and passed handwritten gold through the compiled adapter and the compiled comparator. Value, end-audit, partial status, and missing-summary mutations mismatched or refused.

## Do not treat this as acceptance

B-D02 stays unaccepted. This handoff does not mark a parent item, a question bank, or a release. The live fixture is two roles and a handful of statements. It is not a 300-question run and it does not call a model.

## Failed attempts

Keep `linux/proof/attempts/001` through `006`. They record the type-inference failure, the private `ConfigError::at` call, the missing libclang directory, the stderr Executable line, the peek binding on the two-turn stdout, and the hardlinked `query-pg` inode. The green consoles are `linux/proof/console.log` and `linux/smoke/console.log`.

## Cleanup

`cleanup/run.log` records removal of `oo-a222-pg`, the `oo-a222-eef831aa-*` volumes, network `oo-a222-eef831aa-net`, and `/tmp/oo-a222-eef831aa-dba4-4af2-a78b-541bc9e0a222`. Pinned Rust and PostgreSQL images stay. No prune. Other owners' containers were not removed.

## Next reader

A second live case would be a new run. The comparator and `query-pg` sources in the product commit are untouched. Decimal coverage is the handwritten adapter tests. The saved PostgreSQL stdout has no decimal column.
