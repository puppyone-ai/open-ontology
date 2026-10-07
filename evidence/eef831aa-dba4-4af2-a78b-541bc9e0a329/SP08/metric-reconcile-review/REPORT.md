# SEM-D05 / E42 metric-reconcile review

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a329`. Independent review of the author packet `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a316/SP08/metric-reconcile` at fixed commit `15558917e2c5be12b74dd1d64fb542b7d05a03af` (tree `140fddc79768fd4d1c5b43c4f46ba2acba6cc5bf`).

Verdict: **APPROVE_LIMITED**.

The approval covers the child's calculation readiness on one closed anchor: parse, typed binds, role `EXPLAIN`, a one-row test, and an independent numeric control. `metric_certified: true` in `pass.json` and `scale.json` is that component predicate from `tools/metric-reconcile/CONTRACT.md` and `src/lib.rs` (G5a `passed`, G5b `passed`, enum `certification_allowed`, `owner_review_required` false).

docs/06 §3.7 owner certification (`certified: true` only after owner review), §6 publication (Git tag only when G1–G13 all pass), `owner_authenticated`, and `full_gates` stay unapproved. Those three flags are false on every saved report. Omitted gates G1, G2, G3, G4, and G6–G13 are `not_run`. This review does not accept the parent SP08 goal, does not publish a metric, and does not claim a release. No ledger or spec counts were written.

## What was reviewed

Frozen task `execution/prompts/v01-metric-reconcile-active.md`, `tools/metric-reconcile/CONTRACT.md`, `docs/06-ontology-layer.md` §3.7 and §6, and the author REPORT, FINDINGS, and HANDOFF. The product tree was not edited. The builder archived commit `15558917` only. `linux/review-3/source-diff.txt` is empty against `source/blobs/tools/metric-reconcile`. Child lock `59599e433b3c025a0ec739989c0f9df74eebcc2f6cb9f9a96892523a10048948`. Root lock stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` before smoke, after smoke, and at the end. `cargo update` was not run (`cargo-update-invoked.txt` is `no`). Oracle registry pins are a subset of the root lock (`oracle-lock-drift.txt` is empty).

## Proof

Linux arm64 Rust `1.94.0` (`4a4ef493e 2026-03-02`) image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL 17.10 image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. One builder, 6 GiB, 1 CPU, `cap-drop ALL`, private network `oo-a329-eef831aa-net`. One database, 1 GiB, 1 CPU, no host port, scram, TLS SAN `DNS:oo-a329-pg`. `pg/up.exit` is 0. Author `src/live.rs` was not invoked (`author-live.txt` is `not_invoked`). `OO_A316_LIVE` was unset.

Accepting builder `linux/review-3`, container `oo-a329-review-3`, removed after the run:

- `docker-run.exit` 0, `review.exit` 0
- `OOMKilled=false`, `Privileged=false`, `Memory=6442450944`, `NanoCpus=1000000000`, `NetworkMode=oo-a329-eef831aa-net`
- `oo-gateway` `sql_guard`: 24 passed, exit 0
- `cargo test --locked --offline` for `metric-reconcile`: 21 passed, 0 failed, exit 0
- `cargo clippy --locked --offline --all-targets -- -D warnings`: exit 0
- independent oracle `live.exit` 0, `checks.txt` has no `FAIL` line

The oracle uses its own tables `rev.ticket` and `ops.hand_close`, role `oo_r_review` (NOLOGIN), and login `oo_a329_login` (NOINHERIT). `SELECT` is granted only to the role. A direct `SELECT` as the login returns SQLSTATE `42501`. The component query still returned the row, which is the `SET ROLE` proof. Inserts are separate statements. The hand literal is `1000000000000000000.01`. The oracle subtracts decimal digit strings locally. It does not call `f64` and it does not call the author's live test.

| run | outcome | G5a | G5b | metric | control | difference | metric_certified | owner_authenticated | publication | full_gates | receipt_sha256 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| compile.json | compiled | block not_executed | block not_executed | | | | false | false | false | false | `b8f71cf30412833cdaeeb8067c2bff1e439a1da1b37585f896cbe78e5a3203b0` |
| pass.json | evaluated | passed ok | passed ok | 1000000000000000000.01 | 1000000000000000000.01 | 0.00 | true | false | false | false | `e842613a56638437486fd4a9f9fc943c4c1cf436ec4ad653fc5d7e955c1976b8` |
| tolerance.json | evaluated | passed ok | failed tolerance | 1000000000000000000.01 | 1000000000000000000.00 | 0.01 | false | false | false | false | `54d805782eed8e70793c85e5eff98a6bf96ae339df1d7925a6d589ef1f1e79e4` |
| scale.json | evaluated | passed ok | passed ok | 1000000000000000000.01 | 1000000000000000000.010 | 0.000 | true | false | false | false | `4f48c72f7559a81caa2669f1238dfcafdf5894db19c479b4e6230e279b1b4aab` |

Executed metric SQL on the three evaluates:

```text
SELECT ticket.gross AS gross FROM rev.ticket WHERE ticket.booked_on >= $1 AND ticket.booked_on < $2 AND ticket.kind = $3
```

Executed control SQL:

```text
SELECT hand_close.closed_sum AS closed_sum FROM ops.hand_close WHERE hand_close.anchor_day = $1
```

Those strings contain neither `2026-03-01`, `2026-04-01`, the hand literal, `oo_mr_probe`, nor `810316`. The anchor date `2026-04-01` is a bound value and a provenance field. Explain status `passed`, 2 rows. Test status `passed`, `actual_rows` 1. Role `oo_r_review`. Anchor `last_closed_period_end`, closed. Fingerprints differ: metric `25c86090d5a21b35`, control `fb6b01e2454474b4`. PostgreSQL stored the scale-3 control as the text `1000000000000000000.010`. After the control updates the metric row was still `.01` and both tables still had one row.

Refusals, each exit 2, `database_used` false, `metric_certified` false, executed SQL empty, stderr `metric-reconcile: refused <token>`:

| file | reason | G5a | G5b |
|---|---|---|---|
| refuse-unknown_class.json | unknown_class | block unknown_class | block not_executed |
| refuse-same_path.json | same_path | block same_path | block not_executed |
| refuse-anchor_open.json | anchor_open | block anchor_open | block not_executed |
| refuse-gap_anchor.json | gap_anchor | block gap_anchor | block not_executed |

`gap_anchor` leaves `anchor_date` null. The request YAML has no `certified:` key. Compile does not open the database (`database_used` false, explain `not_executed`).

Password bytes from the private secret files were absent from this evidence tree. The scan ran before cleanup deleted `/tmp/oo-a329-eef831aa-dba4-4af2-a78b-541bc9e0a329`. No `*.pass`, `*.key`, or certificate file was stored under the evidence directory.

## Attempts kept

| dir | result |
|---|---|
| linux/review | smoke exit 0 (sql_guard 24). `cargo test` and clippy exit 101 because the private archive omitted `examples/schema/metrics-finance.yaml`, which `behavior.rs` includes. Live skipped. `docker-run.exit` 101. Not OOM. |
| linux/review-2 | smoke 0, tests 21 passed, clippy 0. Oracle compile exit 101, two `E0382` borrows in the review oracle (`host`, `report.outcome`). Live skipped. Product sources unchanged. |
| linux/review-3 | smoke, tests, clippy, and live exit 0. Accepting raw proof. |
| pg | `up.exit` 0 |

## Recipes

Launch shape follows a204 overlay-3 `78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee` via a316 preoverlay `1d985581ec415312613a7af91d0530e894e757d79294d497fa8580e74b0d5e56`. PostgreSQL follows a200 `up.sh` `96613a159f6c05b4a8d8d1fb79bb2613ae9b3e9a00a7b942196548a00964da45` and `pg-wrap.sh` `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f`. Libclang extract follows a200 `dc0d980692091bec55d4fbc3eb12a0eda9c5aaa4b9f7d5c6eab995b2a726690d` via a316 `025174865bb7d489b7994ffa8301c0dc56283d872e2b366bef933b69af0aa1de`. a199 preoverlay launch `bf2a422d751db818cdd4d852d14be00436d4177cb584b85131a4774453413f55`. Review-3 launch `dd69a34ddec71e9bd014fdd908b0bd64ca4a5b4d8b9b2c0533783703d47d0cf4`, inside `f87dcba7318f20a96c06d00c7112abefce1936a08928e51631f59c125cb77d2d`. The review-3 archive also extracts `examples/schema/metrics-finance.yaml`. `rust-toolchain.toml` from the archive (channel 1.98.1) was moved aside so `RUSTUP_TOOLCHAIN=1.94.0` stayed the compiler.

## Cleanup

`cleanup/run.sh` removed `oo-a329-review`, `oo-a329-review-2`, `oo-a329-review-3`, `oo-a329-pg`, volumes `oo-a329-eef831aa-cargo`, `oo-a329-eef831aa-target`, and `oo-a329-eef831aa-pgdata`, network `oo-a329-eef831aa-net`, and `/tmp/oo-a329-eef831aa-dba4-4af2-a78b-541bc9e0a329`. `cleanup.exit` is 0. `pg_rm`, both volume removals, and `net` are 0. A following `docker ps` / volume / network check showed no `oo-a329` resources. No prune and no other owner's containers.
