# HANDOFF — a108 segment preflight review

Stop. Scoped decision is approve for commit `6cd955af38ce426bb1102a6e078e8580febed776`, tree `463bbf6ada8ea58e676370c872bfd711e9b9c251`, parent `3ca5357930a2c44c42d3061db5fabe577881269a`.

No stage, commit, push, release, or customer access from this session.

## Read first

- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a108/SP04/segment-preflight-review/review.md`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a108/SP04/segment-preflight-review/findings.json`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a108/SP04/segment-preflight-review/pg/summary.log`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a108/SP04/segment-preflight-review/linux/console.log`

Deciding run `06f2ff6d-9e03-40b3-bd34-f4ccf1b9642a`. Linux arm64 Rust `1.94.0 (4a4ef493e 2026-03-02)`. PostgreSQL `17.10 (Debian 17.10-1.pgdg12+1)` aarch64. Probe exit 0 with 154 PASS lines and `fails=0`. Fetch, package test, clippy, and the author live test on the `6cd955` archive exit 0. Live result `ok. 1 passed` in 4.64s. Worst exit 0.

## Closed here

A099-1. Reference path `0003` only returns `OO002` and `target-fence-receipts/1 segment relation is absent` for a missing schema, a missing relation, and a missing segment constraint, with no v3 marker. Frozen `0002` then `0003` on a missing relation returns `42P01` `relation "_oo.apply_receipts" does not exist`. That dead path stays RED. `0000`, `0001`, `0002`, v1 JSON, and v2 JSON stay the frozen SHA-256 values. `0003` is `d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3`.

The reference applicator executed by the product test is `route` / `apply_reference` in `crates/oo-pipeline/tests/segment_preflight_correction.rs`. Production `crates/oo-pipeline/src` does not call it. It is not a Loader.

## Still unsigned

SP04-W05, SP04-W05-a, AT-PL-05, Loader, CDC, control mirror, ACK, physical mapping, 100 workers, dual arch, runsc, customer gate, protected CI, full supply-chain, and release. A085-F2, A085-F3, and A085-F5 stay open. A085-F4 stays an installer obligation: `segment_preflight_ready` compares the stored digest and does not read the file. The 17/163 grid in this probe is a static count, not a new 180-cell PostgreSQL run, and the 320-case matrix was not rerun.

## Retention

Author first fixed Clippy and live 101 stdout exists only as narrative in the a104 REPORT and HANDOFF. This review did not recreate it. a108 harness consoles are under `linux/attempts/` for runs `76dbc1ff`, `d126c4e3`, and `116fb4b9`. The author's 4.70s overlay result is delivery. Approval is run `06f2ff6d` on the git archive.

Owned overlay inputs in `/tmp/oo-a104-eef831aa-dba4-4af2-a78b-541bc9e0a104/fixed-snapshot` compare equal to `6cd955`. The deciding cargo smoke used the `6cd955` lock.

## Cleanup left in place

Removed the deciding Rust container, PostgreSQL container, network, and PostgreSQL volume. Retained `oo-a108-eef831aa-dba4-4af2-a78b-541bc9e0a108-cargo` and `oo-a108-eef831aa-dba4-4af2-a78b-541bc9e0a108-target`. No a108 or a104 containers remained. The a104 cargo and target volume names were already absent from `docker volume ls` before the second a108 launch. This review did not delete them and did not delete the a104 `/tmp` snapshot. Coordinator handles those names.

Plan checkboxes were not edited.
