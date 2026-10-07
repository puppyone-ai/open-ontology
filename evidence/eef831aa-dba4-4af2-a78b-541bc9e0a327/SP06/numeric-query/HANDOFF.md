# Handoff

a327 admits bounded numeric quantity, decodes OID 1700 with the existing exact decimal, and has a private TLS `query-pg` stdout of six decimal rows.

## Approved product SHA

`976cb4538f621eb5de157cef32976b029c6d17cf`.

That commit contains the quantity admission (`c671a3ca`), the bounded allowlist (`e9cba7df`), and the `row_filter: null` request rule. The evidence commit that records live-3 does not change those sources. Other tasks may commit between the product SHA and the evidence commit. Coordinator review and the same a317 100k fixture use a SHA whose six overlay files still match `linux/live-3/overlay-sha256.txt`. Keep a317 decisions, row count, gold, and classification as they are. Do not hand-write the target, the pack, or the query output. This task did not re-run a317.

## What live-3 already showed

Archive `e6ebf93b5ac261c097a24d145611e0300e4f88b5` plus the six-file overlay. Lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. `install-column-acl` exit 0. `provision-mapping-reader` exit 0. Amount query exit 0, six decimal rows, summary `returned_rows` 6. Hidden column exit 6, stderr `query-pg: guard`. Before analyze exit 5, stderr `query-pg: policy`. Filter `--exact a327_live::live_numeric_proof` with `OO_A327_LIVE=1`.

Launch scripts for that run are `linux/live-3/launch.sh` and `inside.sh`. PostgreSQL restart for the clean fixture is `pg/up-live3.sh`. The earlier scripts `linux/live/` and `pg/up.sh` remain the install-exit-7 attempt.

## Seam left outside this task

`crates/oo-gateway/src/mapping_reader_access.rs` `ensure_supported_type` checks the type name and `typtype` `b` only. It does not apply `grantable_base_type`. Unconstrained numeric and precision above 38 can pass provision-mapping-reader. The six-row columns are inside the grant bound, so live-3 provision succeeded. Do not treat that as a bound on mapping-reader. A later task has to own that file before changing it.

## Leave in place

Infinity still cannot be stored in `numeric(18,4)` (`sqlstate` 22003, `linux/live-failed-2`). NaN still dies in `import-pg` with `numeric_nan` (`linux/live-failed-3`). Decoder tests already name those wire steps.

`linux/live-failed-1` exit 0 is a zero-test false green. `linux/postedit/postedit.exit` 2 is the old harness gate. The script `linux/postedit/inside.sh` now accepts the passing unbounded-numeric quantity test. Do not rewrite those exit files. Do not retune a317 to hide a boundary. Do not insert into `_oo` by hand.

Do not claim I01, I02, parent acceptance, or release from this evidence.

## Resources

Private names from this task:

- container `oo-a327-pg`
- containers `oo-a327-live2` and `oo-a327-live3` (launch scripts already removed them)
- hash container `oo-a327-hash` (inventory only, label `oo.kind=hash`, removed after the manifest)
- network `oo-a327-eef831aa-net`
- volumes `oo-a327-eef831aa-cargo`, `oo-a327-eef831aa-target`, `oo-a327-eef831aa-pgdata`
- private root `/Users/redux/puppy-one/oo-a327-eef831aa-dba4-4af2-a78b-541bc9e0a327` (secrets and the archive checkout; Docker cannot bind `/tmp`)

Own cleanup after the evidence commit removes only those names. Other owners' containers stay. No prune.
