# Verdicts

Session `eef831aa-dba4-4af2-a78b-541bc9e0a286`. Independent reading of fixed archive `c551afea55d5a9164760878a5d6a76835b869875`, tree `e03c77fde3be687ace03984569e128407618017a`. The seam commit is `eb0bbbe7b82789cbdc362771e1b89226fc1af832`. The pre-seam adapter parent is `f57e430f51b9f44175ffab5ae876cd64dd558c78`, tree `343ccfaf6d32a07c615f238e6c39e243c6e5fb7b`. This session edited no product file, no root lock, and no bench lock.

B-D07: APPROVE_LIMITED. The question-bank suite on this archive compiles one validated public bank role group into an `oo.benchmark.sql-runner/v1` suite. The fresh hand bank below is the independent proof. A 300-question bank, two-human annotation, a customer holdout, benchmark quality, and a release stay open.

B-D08: APPROVE_LIMITED. This is an extension of the existing query-result adapter, sql-runner, and question-bank suite. It is the review of that extension. It is not a new-component first review. The optional binding field `empty_result` keeps omitted and explicit `rows` byte-identical to the parent adapter, and emits `not_found` only for a clean complete zero-row result under `not_found_if_complete_empty`.

Parent acceptance of the benchmark goal stays open. These two lines are the whole of this review.

# Scope

Owned evidence is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a286/SP12/question-bank-suite-review/**`.

The suite compiler reads one bank, checks the manifest hash, keeps one role and one `as_of`, and writes the runner suite. Unsupported expected kinds refuse the whole compile. `needs_reannotation` stays in the denominator. `quality_claim` is `none`, `model` is `none`, and `authentication` is `not_claimed`.

`empty_result` is optional on the adapter header. Omitted and `rows` keep the empty-table bytes. `not_found_if_complete_empty` emits `{"kind":"not_found","completion":"complete","warnings":[]}` only when the result has zero rows, `returned_rows` 0, `max_rows` greater than 0, status `succeeded`, `end_audit_ok` true, `rollback_confirmed` true, `socket_dropped` false, `stop` null, `sqlstate` null, and `truncated` false. A truncated, failed, or `max_rows` 0 zero-row result stays an empty table. A nonempty result stays rows. The adapter does not read gold. sql-runner copies the binding header string and does not take `empty_result` from gold. The suite compiler sets the opt-in only on `not_found` questions.

# Archive and locks

`linux/smoke/seam-marker.txt` records `fixed=present parent=absent` for `not_found_if_complete_empty` in the adapter parser. `seam-ancestor.exit`, `fixed-ancestor.exit`, `root-lock-since-suite.exit`, `bench-locks-since-suite.exit`, `suite-lock-since-introduction.exit`, `seam-lock-diff.exit`, `adapter-runner-lock-vs-parent.exit`, and `head-vs-fixed-benches.exit` are 0. `seam-lock-names.txt` is empty. Linux `cmp` of the adapter, sql-runner, and suite locks against the parent archive is 0.

`tools/lock_subset.rs` read the adapter, sql-runner, question-bank suite, question-bank, and result-comparator locks. `linux/smoke/lock-subset.stdout` ends `summary registry_triples=575 missing=0 mismatch=0`. Each lock contributes 115 registry triples. Path packages are printed and are not required in the root lock. Serde is 1.0.229 checksum `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba`. Serde_json is 1.0.151 checksum `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14`.

Repo HEAD recorded at the green smoke launch is `2a6b6f0acd51c060577e1f336e0a686d54df2001`. A later check still has `c551afea` as an ancestor of `8e363e1bc8d99a16825a450824f7b48542f12fb8`. The binaries were built from the immutable archive, with `rust-toolchain.toml` renamed aside so rustc stayed 1.94.0.

# MSRV smoke

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64. Container `oo-a286-smoke` id `b415a3b60808319d01d969e6ad3f8e865bcf970c57c3a7bcd4bfdf5b3c13adec`. Memory 6442450944, NanoCpus 1000000000, CapDrop ALL, privileged false, network `oo-a286-eef831aa-net`, OOMKilled false, exit 0. `linux/smoke/docker-run.exit` is 0. `linux/smoke/capacity.txt` is `own=0 global=1 host_cargo=0`.

`rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Focused tests, all `--locked --offline -- --test-threads=1`:

- `benchmarks/query-result-adapter` exit 0
- `benchmarks/sql-runner` exit 0
- `benchmarks/question-bank-suite --test cli --test compile --test sha256` exit 0

`cargo clippy --version` exited 1 because clippy was absent from the image toolchain. `rustup component add clippy --toolchain 1.94.0` exited 0. Clippy `--all-targets -- -D warnings` then exited 0 for the adapter, sql-runner, and question-bank suite.

Release builds of sql-runner, query-result-adapter, result-comparator, and question-bank-suite exited 0, offline. The parent adapter build exited 0. Digests are in `linux/smoke/bin-sha256.txt`. The question-bank-suite binary is `27ad7342690bd3c101afc966d3873dfb50a44861441f40e5e9cd7cc423d092d9`. The seam adapter is `3313d9321d689b6f70ab07389411f55d7359939c30ac66e48452e4db14210df1`. The parent adapter is `7934ac366f12909b713c105f0ab8008b65a93c68c8730533d5894164ae21704c`.

`cargo test --test proof` exited 0. That frozen test uses a separate query double at `/target/a286-proof/query-pg` and asserts the runner gate exit 1. It is a regression of the author fixture. The fresh bank is a different document.

Bench tree hashes are `linux/smoke/source/adapter.sha256`, `runner.sha256`, and `suite.sha256`. Eleven seam blobs and `linux/smoke/blob-sha256.txt` are under `source/blobs/`.

# Fresh hand bank

`oracle/fresh_bank.rs` admits the bank with `oo_question_bank::evaluate`, then runs the compiled suite compiler, sql-runner, adapter, and comparator. The query program is `oracle/query_pg.rs`, a Safe Rust double. Its lock was generated offline inside the container: `oracle/Cargo.lock`, `fresh-lock-offline.exit` 0.

The bank role is `review_north`. The excluded role is `review_south`. Question `as_of` and selection `as_of` are `2026-10-06T00:00:00+08:00`. Tuple `as_of` is the date `2026-10-06`. Dataset `oo-review`, suite `public-review`, distribution `public_synthetic`, k is 2. Five questions are planned: `rv-table`, `rv-scalar`, `rv-topk`, `rv-absent`, `rv-note`. `rv-south` is excluded. `rv-note` stays `needs_reannotation`. The others are `score`.

`linux/smoke/fresh-bank.exit` is 0. Stdout markers include `scorer_agent_separated=1`, `bad_decimal=bank`, `top_level_null=schema`, `mixed_as_of=group`, `unsupported_whole_compile=1`, `flipped_hash=refused`, `template_empty_result=schema`, `null_cell_copied=1`, `reannotation_kept=1`, `selected_role_as_of=1`, `empty_gold_not_reinterpreted=1`, `denominator_5x2=1`, `runner_header_preserved=1`, and `fresh_bank_ok`. The validated manifest hash is `e2fe024fa54aced6ebb84831260c88ee7ff6237155731ad1022b4e1b0739d2d6`.

The compiler report is `oo.benchmark.question-bank-suite-report/v1`, `question_count` 5, `trial_count` 10, `quality_claim` `none`, `model` `none`, `authentication` `not_claimed`. Planned rows are `id`, `index`, and `annotation`. `as_of` is on `selection`.

`questions/rv-absent/gold.json` expected kind stays `not_found` with completion `complete` and empty warnings. It has no rows. The absent binding `empty_result` is `not_found_if_complete_empty`. The table binding omits `empty_result`. The table gold comparator is `unordered` and the decimal coefficient is `450` at scale 2. The query double sent gateway text `4.50`. `case-rv-table/trial-0/adapter.stdout` contains `"coefficient":"450"` and does not contain `4.50`. `case-rv-absent/trial-0/adapter.stdout` is `{"kind":"not_found","completion":"complete","warnings":[]}`.

The runner report is `oo.benchmark.sql-runner-report/v1`: `question_count` 5, `trial_count` 10, `pass_count` 8, `fail_count` 0, `reannotation_count` 2, `not_run_count` 0, `passed` false. Tuple `model` is `none` and `harness` is `sql-runner`. The oracle required runner exit 1 and empty stderr, which is the failed gate while the reannotation trials remain. `fresh-bank.exit` 0 means that assertion held.

A bad coefficient `0450` refuses as `bank` before the executable directory is created. A top-level JSON null is `schema`. Mixed `as_of` is `group` at `/selection/as_of`. `refuse_permission` refuses the whole compile. A nibble-flipped manifest hash is refused. A template `empty_result` is `schema` at `/templates/review_north/binding_header/empty_result`. An admitted null cell is copied as `"presence":"null"`.

# Byte compatibility

`oracle/boundary.rs` drove the built seam adapter and the built parent adapter. `boundary.exit` is 0. Stdout records omitted matches the parent, explicit `rows` matches omitted, the parent refuses `empty_result`, a clean opt-in is the `not_found` object, and truncated, failed, and `max_rows` 0 zero-row results match the parent empty table and are never `not_found`. A contrary nonempty row under the opt-in stays rows. The same three header cases on the captured live zero-row query stdout are in `linux/live/live-bytes.stdout`: `live_omitted_matches_old=1`, `live_rows_matches_old=1`, `live_opt_in=not_found`, `live_bytes_ok`.

# Live TLS

PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, linux/arm64. Container `oo-a286-pg` id `0f1f591947a74d2c99201d898685efdcd2e82f08ea7063325b2f1f7140b7a8db`. Memory 1073741824, NanoCpus 1000000000, CapDrop ALL, added CHOWN, DAC_OVERRIDE, FOWNER, SETUID, SETGID, privileged false, no host port, `pg/up.exit` 0.

Live builder `oo-a286-live` id `724ae15de0e81412ea95d32b869e70e8f05620f5d0131563f3e1993508bdecbf`. Same Rust image and 6 GiB / 1 CPU limits. `docker-run.exit` 0, OOMKilled false. `capacity.txt` is `own=0 global=0 host_cargo=0 own_pg=1`. `query-pg` offline fetch exited 101 on the empty package set, the online fetch exited 0, and the offline build exited 0. `cargo rustc --example a286_probe -- -D warnings` exited 0. `Cargo.lock` and `crates/oo-gateway/Cargo.toml` were unchanged (`lock-unchanged.exit` 0). The example was copied into the writable archive only. `query-pg` digest `33ff89a341c13284b849786aa1727f2f5efe4ac00671a7dcbcb47f4a5384c301`.

The probe installed the gateway column ACL, then ran `SELECT v FROM review.cell WHERE v = 9` and `WHERE v = 4` through the real `query-pg` binary, the seam adapter, and the result comparator. `probe.exit` is 0. `secret-scan.exit` is 0. `oracle.txt` records zero-row query exit 0, adapter exit 0, comparator exit 0, verdict `match`, `returned_rows` 0, and the contrary row query exit 0, adapter exit 0, comparator exit 1, verdict `mismatch`, kind `rows`, `returned_rows` 1. The zero-row summary is `succeeded`, `truncated` false, `returned_rows` 0, `rollback_confirmed` true, `socket_dropped` false, `end_audit_ok` true, `stop` null, `sqlstate` null. Zero adapter stdout is the `not_found` object. Nonzero adapter stdout is rows with int64 value `4`. The comparator mismatch path is `kind`, actual `rows`, expected `not_found`. Libclang extract exited 0.

# Preserved oracle runs

Product tests, clippy, binaries, the lock subset, boundary, and the author proof were already green on the runs below. Each failed console is kept.

- `linux/smoke/attempt-001`: `lock_subset` rustc E0119, then fresh-bank rustc E0308.
- `linux/smoke/attempt-002` and `attempt-003`: bank `as_of` rejected `Z` and `+00:00`.
- `linux/smoke/attempt-004`: gold cells used gateway `kind` objects. The bank requires `presence`.
- `linux/smoke/attempt-005`: the oracle expected `planned[].as_of`. Planned rows have `id`, `index`, and `annotation`. Selected `as_of` is `selection.as_of`.

The green smoke is `linux/smoke/`. The green TLS proof is `linux/live/`.

# Limits

The fresh bank is six authored questions and five selected trials at k=2. It is a fixture. Provenance annotators are `unassigned`. The controlled query double covers the ten-trial denominator. The TLS proof is one clean zero-row select and one contrary row. Truncation, failure, and `max_rows` 0 are the adapter boundary plus the live zero-row header replay. Benchmark quality, model identity, human gold, a customer holdout, and a release are outside this verdict.

# Resources

Builders used 6 GiB and one CPU. PostgreSQL used 1 GiB and one CPU. One builder and one private database ran at a time. Cleanup of the owned containers, network, volumes, and `/tmp/oo-a286-eef831aa-dba4-4af2-a78b-541bc9e0a286` is recorded in `CLEANUP.txt`.
