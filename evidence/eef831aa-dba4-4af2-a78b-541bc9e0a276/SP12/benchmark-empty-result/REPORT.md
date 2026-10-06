# Verdict

Session record for `eef831aa-dba4-4af2-a78b-541bc9e0a276`. The optional `empty_result` seam is in the three owned benches, on fixed archive `8a6395aef5b1d3b16d1b3d72833d2df2ebaa536e`. This session wrote the seam and read its own Linux logs. A later independent review is still required before B-D07 can be accepted. The parent benchmark goal and a release stay open.

# Scope

Owned paths are `benchmarks/query-result-adapter/**`, `benchmarks/sql-runner/**`, `benchmarks/question-bank-suite/**`, and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a276/SP12/benchmark-empty-result/**`.

The product diff against the fixed archive is eleven files. Protocol ids stay `oo.benchmark.query-result-adapter/v1`, `oo.benchmark.sql-runner/v1`, and `oo.benchmark.question-bank-suite/v1`. Root `Cargo.toml` and root `Cargo.lock` are outside this change. a207 remains the only writer of the connector manifest and the root lock. The result comparator, question bank, stats, and `oo-gateway` sources were left unchanged.

# Contract

The adapter header may carry `empty_result`. Omitted, and the explicit word `rows`, keep today's empty-table bytes. `not_found_if_complete_empty` emits `{"kind":"not_found","completion":"complete","warnings":[]}` only when the gateway result has zero rows, `returned_rows` is 0, `max_rows` is greater than 0, status is `succeeded`, `end_audit_ok` is true, `rollback_confirmed` is true, `socket_dropped` is false, `stop` is null, `sqlstate` is null, and `truncated` is false. A non-empty result stays `kind` `rows`. A zero-row `failed`, `partial`, or `truncated` result, and a zero `max_rows` budget, stay an empty table. `succeeded` with `end_audit_ok` false is still a completion refusal. A null, a non-string, or any other string is schema at `/0/empty_result`. The adapter does not read expected gold.

The sql-runner binding schema accepts the same optional string and writes it back unchanged. The runner does not take the value from gold, from an expected document, or from the comparator.

The suite compiler sets `not_found_if_complete_empty` only on the binding of a question whose kind is `not_found`. Gold `kind` `not_found` stays as declared. Every other question omits the field. A template `binding_header` that already contains `empty_result` is schema. `oo_core::parse_json_value` accepts JSON null. A null at the top of a checked object is schema, including a top-level null at `/`. A null cell inside bank-admitted gold is copied.

# Proofs

All of these runs used image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, rustc 1.94.0, linux/arm64, 6 GiB, 1 CPU, `--cap-drop ALL`, and a private network. `fixed-ancestor.exit` is 0 on the smoke, post, and live launches.

Pre-edit smoke, `linux/smoke/`, tested the three suites on the fixed archive before the overlay. `test-adapter.exit`, `test-sql-runner.exit`, `test-suite-focused.exit`, `test-proof.exit`, and `docker-run.exit` are 0.

The red pipeline in `linux/smoke/red/` ran the fixed adapter, the fixed runner, and the compiled bank. Adapter stdout is `{"kind":"rows","completion":"complete","warnings":[],"columns":[],"rows":[]}`. Comparator verdict is `mismatch` on path `kind`, actual `rows`, expected `not_found`. The report has `question_count` 5, `trial_count` 10, `pass_count` 6, `fail_count` 2, `reannotation_count` 2, and `passed` false. `red-runner.exit` is 1. The red adapter sha256 is `83f00f5b168656d941601c48b8b750debfce4f8b36bf88ce0a0ea26a3cd45b5d`.

Post-edit `linux/post/` formatted, retested, and ran clippy. Adapter, sql-runner, suite-focused, and proof tests exited 0. Clippy `-D warnings` on the three crates exited 0. The green runner exited 1, which this gate requires. The report has `question_count` 5, `trial_count` 10, `pass_count` 8, `fail_count` 0, `reannotation_count` 2, and `passed` false. Both `pub-absent` trials are pass / match. `linux/post/green/adapter.stdout` is the `not_found` object. The table adapter stdout still contains coefficient `120` and does not contain `1.20`. The post adapter sha256 is `3313d9321d689b6f70ab07389411f55d7359939c30ac66e48452e4db14210df1`. The comparator sha256 is `fccc84374ddafa93f247587b2d8c36035c1d42c07bdbf7e032eef81f12313914`. The `query_pg` sha256 in that report, `29c156ad867c69e0d078ec42d3fc4c8c566657af4fc199bfcfbad902a3d44642`, is the suite test double.

The adapter unit `opt_in_emits_not_found_only_for_a_clean_zero_row_result` covers the clean zero-row opt-in, a non-empty opt-in that stays `rows`, and the zero-row breaks: `failed`, `partial`, `truncated`, `rollback_confirmed` false, `socket_dropped` true, `stop` `timeout`, `sqlstate` `P0002`, and `max_rows` 0. Truncated completion stays `complete`. `succeeded` with `end_audit_ok` false still refuses at `/summary/end_audit_ok`. `invalid_empty_result_is_schema` refuses null, `1`, `true`, `{}`, `[]`, `""`, `not_found`, and `ROWS`. `current_fixture_headers_stay_empty_tables` covers `desk-header.jsonl`, `peek-header.jsonl`, and `count-header.jsonl`.

Live TLS is `linux/live/`. Postgres 17.10 image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` ran as `oo-a276-pg` with no host port. The fixture is database `oo_a276`, table `probe.slot`, one `int8` column `n`, and one row whose value is 1. It is a purpose-built probe. It is not a business table and it is not an authentication claim.

`query-pg` was built `--locked` from the fixed archive. Its sha256 is `06f915f7c2fc0c0d477c31d4906c7f135092ed625e8a08acc907d0e21848a94e`. The evidence example sha256 is `b3082d2a4d0e00d9bf90d676b3e52cc9fbca68534fce633222afc176b8f356da`. The reused adapter and comparator match the post binaries. `Cargo.lock` and `crates/oo-gateway/Cargo.toml` were unchanged in the archive (`lock-unchanged.exit` 0).

`SELECT n FROM probe.slot WHERE n = 2` returned `returned_rows` 0 with a clean summary. The adapter stdout is the `not_found` object. The comparator verdict is `match` and its exit is 0. `SELECT n FROM probe.slot WHERE n = 1` returned `returned_rows` 1. The adapter kind is `rows` and the cell value is `1`. The comparator verdict is `mismatch` on path `kind` and its exit is 1. `probe.exit` is 0. `secret-scan.exit` is 0. Container `d4444e2fd7594aaa4901d0d424c0bd503f1bdd87c15318588c8f1f7bb0e642f9` exited 0, OOMKilled false, privileged false, memory 6442450944, NanoCpus 1000000000.

Two earlier live builds are kept. `linux/attempts/001/` exited 101 because `pg_query` bindgen had no `libclang` (`bc15769928c8c2b6cd585ed3ebe439a57393327bbecb31ab5fb2a9976d2efa25`, OOMKilled false). `linux/attempts/002/` compiled `query-pg` and exited 101 on the example warning that `Slot.column` was never read (`aa2d1ae6030f2ce00a0d675849c1a2908b0381b9dfdc8c972151c9dc4c8c3f67`, OOMKilled false). The third build dropped that field and is the live proof above.

# Limits

The bank pipeline's query program is the suite test double. The TLS proof is one zero-row select and one contrary row through the real `query-pg` binary. The zero-versus-truncated, failure, and `max_rows` 0 matrix is the adapter unit above. This session does not mark B-D07 accepted and does not treat the hand bank as a 300-question bank.
