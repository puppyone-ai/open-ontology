# Handoff

a327 admits bounded numeric quantity, decodes OID 1700 with the existing exact decimal, and has a private TLS `query-pg` stdout of six decimal rows.

## Approved product SHA

`21cc96241a841b47dbbcb116549030e38a643ba0`.

That commit is the mapping-reader typmod bound on top of the quantity admission (`c671a3ca`), the bounded allowlist (`e9cba7df`), and the `row_filter: null` rule (`976cb453`). The frozen addendum is `9183e79082341a1a2bd7d3b1a2ae3e8764735d0e`. Parent of the product commit is `e7fb67e27bcffcdeec10ad84463b4854bef6eed0`. Other tasks may commit between the product SHA and the evidence commit. Coordinator review and the same a317 100k fixture use a SHA whose seven overlay files match `linux/live-4/overlay-sha256.txt`. The six live-3 hashes still match except `query-pg.rs` and the added `mapping_reader_access.rs` line. Keep a317 decisions, row count, gold, and classification as they are. Do not hand-write the target, the pack, or the query output. This task did not re-run a317.

## What live-3 already showed

Archive `e6ebf93b5ac261c097a24d145611e0300e4f88b5` plus the six-file overlay. Lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. `install-column-acl` exit 0. `provision-mapping-reader` exit 0. Amount query exit 0, six decimal rows, summary `returned_rows` 6. Hidden column exit 6, stderr `query-pg: guard`. Before analyze exit 5, stderr `query-pg: policy`. Filter `--exact a327_live::live_numeric_proof` with `OO_A327_LIVE=1`.

Launch scripts for that run are `linux/live-3/launch.sh` and `inside.sh`. PostgreSQL restart for the clean fixture is `pg/up-live3.sh`. The earlier scripts `linux/live/` and `pg/up.sh` remain the install-exit-7 attempt.

## Mapping-reader bound, closed in this packet

`ensure_supported_type` now uses `grantable_base_type(typname, atttypmod)`. `linux/live-4` refused provision of imported unbounded numeric and `numeric(39,4)` with exit 7 and stderr `provision-mapping-reader: policy type` before any mapping grant. `numeric(18,4)` and `numeric(38,4)` still provision, and the six-row decimal stdout is unchanged. Launch scripts are `linux/live-4/launch.sh` and `inside.sh`. PostgreSQL start is `pg/up-live4.sh`. Pre-edit smoke is `linux/smoke-map`. The two bind failures `linux/smoke-map-failed-1` and `linux/smoke-map-failed-2` stay.

Remaining live effects, left as they are: `whitelist_tokens_ok` and `load_types` do not apply precision. `supported_base` also requires the `pg_catalog` type OID. Install and mapping-reader do not repeat that OID check. `COMPARISON_SIGNATURES` still omit `numeric`, so a bounded numeric column can be selected and has no comparison, literal, or aggregate. No other live Rust file reads `SUPPORTED_TYPES`.

## Leave in place

Infinity still cannot be stored in `numeric(18,4)` (`sqlstate` 22003, `linux/live-failed-2`). NaN still dies in `import-pg` with `numeric_nan` (`linux/live-failed-3`). Decoder tests already name those wire steps.

`linux/live-failed-1` exit 0 is a zero-test false green. `linux/postedit/postedit.exit` 2 is the old harness gate. The script `linux/postedit/inside.sh` now accepts the passing unbounded-numeric quantity test. Do not rewrite those exit files. Do not retune a317 to hide a boundary. Do not insert into `_oo` by hand.

Do not claim I01, I02, parent acceptance, or release from this evidence.

## Resources

Private names from this task:

- container `oo-a327-pg`
- containers `oo-a327-live2`, `oo-a327-live3`, `oo-a327-live4`, and `oo-a327-smokemap` (launch scripts remove the builders; PostgreSQL stays until own cleanup)
- hash container `oo-a327-hash` (inventory only, label `oo.kind=hash`, removed after the manifest)
- network `oo-a327-eef831aa-net`
- volumes `oo-a327-eef831aa-cargo`, `oo-a327-eef831aa-target`, `oo-a327-eef831aa-pgdata`
- workspace build root `/Users/redux/puppy-one/open-ontology/.a327-work` (archive, live-work, secrets). Docker Desktop does not bind the recreated sibling `/Users/redux/puppy-one/oo-a327-eef831aa-dba4-4af2-a78b-541bc9e0a327`. That sibling may still exist from the failed smokes and is removed in own cleanup when the name matches.

Own cleanup after the evidence commit removes only those names. Other owners' containers stay. No prune.
