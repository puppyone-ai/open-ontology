# Independent review — PostgreSQL lossless types

Session `eef831aa-dba4-4af2-a78b-541bc9e0a110`. Reviewer of fixed commit `3ca5357930a2c44c42d3061db5fabe577881269a` (tree `ec11cd3ee7948c33e6d43cd2e984e01b948f94e6`, parent `dd5ecf39604b8467a0d1f91d56d2e90473fa2c48`). Subject: `connectors: decode PostgreSQL values into Arrow 60 batches`. Committer time `2026-10-02T21:30:04+08:00`, adoption instant used for age `2026-10-02T13:30:04Z`.

Verdict: **request_changes**.

The deciding command for that verdict is `cargo deny --config ci/deny.toml --locked check`, exit 4, commit `3ca53579`, scope the workspace graph selected by `ci/deny.toml`. `licenses FAILED` on new `tiny-keccak` 2.0.2 `CC0-1.0` and on inherited `webpki-root-certs` 1.0.9 `CDLA-Permissive-2.0`.

The production decode path is approved inside the narrower scope in the sections below. That scope does not sign the parent Reader, Loader, SP03-W04, AT-SRC-03, AT-SRC-05, the vertical, amd64, or a release.

## What was reviewed

The compile input is `snapshot/`, a `git archive` of `3ca53579` (9722 files, names matched `git ls-tree`). The reviewer probe was copied only into that snapshot: `types/review_wire.rs`, `tests/independent_gold.rs`, `tests/log_audit.rs`, and one `mod review_wire` line. `Cargo.lock` in the snapshot stayed sha256 `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f`. `ci/deny.toml` stayed sha256 `4320807bfe747fe339ce7bf539a1cff5604e8e0c7677760e2cbc7abc1868624b`. HEAD was not moved. No product, lock, manifest, policy, Spec, or Plan file was edited. Author a098 evidence, including `pg/oracle.py` and `pg/out/oracle.json`, was not deleted. The new Rust oracle is this session's `independent_gold` and `review_wire`, not that Python run.

Author a098 (`eef831aa-dba4-4af2-a78b-541bc9e0a098`) delivered `REPORT.md` and `HANDOFF.md` and a container exit 0. Their package test was exit 101 for a missing `OO_PG_FIXTURE`. Their reader log has four `oo-pg-source` connection resets and no statement text. This review does not treat that log as zero-write proof.

## Commands and exits

Final builder: image `rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. PostgreSQL image `postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, server `17.10 (Debian 17.10-1.pgdg12+1)`, `server_version_num` 170010. Run id `8e78193a-5ffb-4d8a-9ba8-9f26e2732e6b`. Fixture logging was switched to `log_statement=all` and `log_min_duration_statement=0` only after roles and seed, then marked with `SELECT 'A110-LOG-READY'`. The postgres command line did not set `log_statement`.

Final phase exits, from `raw/phases/exits.txt`:

| Phase | Exit |
| --- | --- |
| `rustc -vV` | 0 |
| `rustup component add clippy` | 0 |
| `cargo fetch --locked` | 0 |
| `cargo test -p oo-connectors --lib --locked --offline` | 0 (61 passed, 0 failed) |
| `cargo test -p oo-connectors --locked --offline` | 101 |
| `cargo clippy -p oo-connectors --all-targets --locked --offline -- -D warnings` | 0 |
| `cargo test -p oo-connectors --test independent_gold --locked --offline -- --ignored` | 0 |
| `cargo test -p oo-connectors --test log_audit --locked --offline -- --ignored` | 0 |
| `cargo install cargo-audit --version 0.22.2 --locked` | 0 |
| `cargo install cargo-deny --version 0.20.2 --locked` | 0 |
| `cargo audit --version` | 0 (`cargo-audit-audit 0.22.2`) |
| `cargo deny --version` | 0 (`cargo-deny 0.20.2`) |
| `cargo audit --json` | 0 (0 vulnerabilities, 1280 advisories in the database) |
| `cargo deny --config ci/deny.toml --locked check` | 4 |
| `cargo tree` probes below | 0 |

Earlier probe-only failures are kept under `raw/attempts/1` through `raw/attempts/5`. Attempt 1 ran `cargo deny check --config ci/deny.toml` and exited 2 because cargo-deny 0.20.2 takes `--config` before the subcommand. That exit is a CLI placement error. The license result is the exit 4 above. Attempt 1 also failed to compile `review_wire` because the helper `column` shadowed itself. Later attempts fixed the probe only.

`cargo test -p oo-connectors` ran the 61 library tests, ignored `independent_live_gold`, and failed `live_source_catalog` at `crates/oo-connectors/tests/live_pg.rs:58` with `OO_PG_FIXTURE is required: NotPresent`. `raw/phases/package-panic-is-missing-fixture.txt` is `yes`. This command is unsigned. It is not a package pass. `OO_PG_FIXTURE` was not pointed at the types fixture, and the catalog lab was not recreated.

## Production batch

`oo-connectors` depends on `arrow-array`, `arrow-buffer`, and `arrow-schema` at `=60.0.0` with `default-features = false`. `TypedBatch::record_batch` returns `arrow_array::RecordBatch`. `lib.rs` has `#![forbid(unsafe_code)]`. A search of `crates/oo-connectors/src` finds that attribute and no `unsafe` block.

`cargo test -p oo-connectors --lib` passed 45 crate tests plus these 16 `review_wire` tests, which compare hand-built wire and literals with the production decoder:

- base-10000 weight: one digit `1` at weight `1` is coefficient `10000`; digit `5000` at weight `-1` is coefficient `5`, plain `0.5`
- negative numeric sign `0x4000` with no digits is unsigned zero
- `numeric(4,6)` wire digit `12` weight `-1` dscale `6` is plain `0.001200` with metadata `oo.pg.decimal=exact`
- canonical hash uses `oo_core` `encode_value` of an `ExactDecimal` literal, and that digest differs from the Arrow buffer preimage
- domain result OID accepts the outer OID and the ultimate base, and rejects the intermediate domain OID
- a self-cycle is `domain_cycle`; a missing base is `domain_base_missing`; a two-node cycle stops as `type_depth`
- a user type whose send function is named `boolsend` stays `user_defined_type`; `money` / `cash_send` is `money_locale`
- array lower bound `0` stays coefficient `0`; a bad element OID and a 3×100 dimension product are rejected
- NaN and infinities reject with `value_len` 8 and no sample in `Debug`
- text keeps an embedded NUL; invalid UTF-8 rejects as `charset` and `Debug` does not contain the sample token
- `time` `24:00:00` (`86400000000`) is `time_out_of_range`; date infinity is `date_infinity`
- a semicolon is not a quoted identifier

The live test does not call `exact_decimal` or `pg_wire_oracle`. Expected Arrow values are SQL literals. It passed, including:

- two reads of `lab.gold` id 1 have equal canonical hashes, different from id 2
- `record_batch()` succeeds; registration is allowed; write state is `read_only`; definers are empty
- catalog `units` of `numeric(10,-2)` reports precision 10 and scale 2046, while the Arrow value is `Decimal128(10, -2)` coefficient 12
- anchor column `n` value 1200 has the same Arrow type and coefficient 12; label is `雪`
- live place hex `00010001000000000001` (digit 1, weight 1) is coefficient `10000`, which a mirrored decoder that returns the raw digit would miss
- `char(3)` `'ab'` arrives as `ab ` (hex `616220`) and is preserved
- jsonb hex is version byte plus text `{"a": 3, "z": 1}` and the Arrow text is the compact form
- domain plans keep the outer OID (`flag_domain` 16392, `d_outer` 16396) and decode as bool OID 16 and int4 OID 23
- array canonical lower bounds stay in the logical struct; Arrow storage is one flat list
- rejects for numeric NaN, date infinity, `time` `24:00:00`, money, composite, and range keep the row; money is absent from the Arrow schema; NOT NULL reject fields are schema-nullable
- `FailRead` returns fact `fail_read`
- one-table `max_rows=1` on `lab.budget_rows` returns fact `max_rows`
- wrong CA and wrong hostname fail at connect with `SourceErrorKind::Tls` and fact `tls_handshake`
- role `oo_a110_guc` (`work_mem=8MB`) returns fact `startup_options`
- role `oo_a110_alias` (`SET role`) returns fact `session_user_mismatch`

`numeric_parts` still returns the low 16 bits unsigned. For `numeric(10,-2)` that scale is 2046. `numeric_typmod_v1` sign-extends 11 bits and returns scale `-2`. The live catalog observation matches the frozen helper. The helper was not edited.

`time '24:00:00'` is an explicit masked reject `time_out_of_range`. The live row stays, with a nullable Arrow field. That bound is the behavior under test.

`fetch_limit` calls `fetch_all` and then rejects a length above `max_rows`. A client timeout sets `poisoned` and returns `statement_timeout`. This review did not see a PostgreSQL `CancelRequest`, and it did not show that the byte budget refuses work before the driver allocates the fetched rows. Those two claims stay unsigned. `PgSourceConfig::try_new` also rejects a selection whose table count exceeds `max_rows` with fact `selection_budget`, before connect.

Arrow consumers of a flat list do not recover per-row lower bounds from Arrow metadata. The lower bounds and lengths are in the canonical struct. This approval covers that split. It does not say Arrow metadata alone preserves array shape.

## Reader trace

The retained log is `raw/pglog/postgresql.log` (also `raw/phases/reader.log`), 898064 bytes. It is cumulative for this fixture: several probe attempts ran before the passing gold test, all as `app=oo-pg-source` after the marker. `log_audit` counts lines with `app=oo-pg-source` and `user=oo_a110_reader`, and takes the SQL after `statement:` or `execute ...:`.

Result, exit 0:

```
reader_sql=782
select=738
start=22
rollback=22
forbidden=0
unknown=[]
canary=0
touch_canary=0
password_hits=0
bytes=898064
```

Sampled reader text includes `START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY`, `SELECT pg_catalog.set_config(...)`, and catalog `SELECT`s. `SELECT set_config` is counted as `SELECT`. Forbidden first keywords were absent. After the marker, `CANARY` and `touch_canary` are absent. The reader password is absent.

Before the reader, `lab.gold` md5 was `9ddf6520d9c8e1f78b2dbbdd1c5242cc`, sink count 0, `anchor` inserts 2, `gold` inserts 2, and all listed updates and deletes were 0. After the passing run those figures are the same (`raw/admin/pg-admin/after-stats.txt`). `xact_rollback` is expected to rise because the reader rolls back; tuple counters and the md5 are the write check.

The first SSL smoke in `raw/admin/pg-admin/ssl.out` failed because the password file was mode 600 and owned by root (`fe_sendauth: no password supplied`). The retry after `chown` to `postgres` exited 0. `nonsql.exit` 2 is the intended `hostnossl` rejection. Passwords were not passed on argv and are not in the evidence files.

## Lock delta

Parent lock sha256 `40664b41001d01db59d272e0d9fab3977842becc88998bd52eefaa0c1b7a7147` (72186 bytes). Commit lock sha256 `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f` (77182 bytes), blob `15ae1071c7236a956547e3f1d37d627b2cbebfce`.

Added, none removed:

`android_system_properties` 0.1.6, `arrow-array` 60.0.0, `arrow-buffer` 60.0.0, `arrow-data` 60.0.0, `arrow-schema` 60.0.0, `const-random` 0.1.18, `const-random-macro` 0.1.16, `crunchy` 0.2.4, `half` 2.7.1, `iana-time-zone` 0.1.65, `iana-time-zone-haiku` 0.1.2, `libm` 0.2.16, `num-bigint` 0.5.1, `tiny-keccak` 2.0.2, `windows-core` 0.62.2, `windows-implement` 0.60.2, `windows-interface` 0.59.3, `windows-result` 0.4.1, `windows-strings` 0.5.1.

Changed stanzas that already existed: `ahash` 0.8.12 gained `const-random`; `chrono` 0.4.45 gained `iana-time-zone` and `windows-link`; `num-traits` 0.2.19 gained `libm`; `fraction`, `num`, and `num-rational` now name `num-bigint` 0.4.8; `oo-connectors` gained the three Arrow crates. `git diff --stat` of `Cargo.toml`, `ci/deny.toml`, `crates/oo-cli`, and `crates/oo-sdk` between parent and commit is empty.

Host `cargo tree -p oo-connectors --locked --offline --edges normal` includes `arrow-array`, `arrow-buffer`, `arrow-data`, `arrow-schema`, `half`, `libm`, `iana-time-zone`, `num-bigint` 0.5.1, and the pre-existing `num-bigint` 0.4.8. On that tree, `ahash` uses `getrandom` (`runtime-rng`). `const-random` and `tiny-keccak` are absent. `cargo tree -e all -i tiny-keccak@2.0.2` and `--edges normal -i tiny-keccak@2.0.2` print `nothing to print` on the host target. `cargo tree --target all -e all -i tiny-keccak@2.0.2` shows `tiny-keccak` under `const-random-macro`, `const-random`, `ahash` feature `compile-time-rng`, and `arrow-array`. That matches `arrow-array` 60.0.0 `Cargo.toml`: the non-wasm `ahash` dependency enables `runtime-rng`; the `wasm32` dependency enables `compile-time-rng`.

`android_system_properties`, `iana-time-zone-haiku`, `crunchy`, `const-random`, `tiny-keccak`, and the five `windows-*` crates are in the lock and are outside the host normal tree above. Their license expressions are in the allow list except `tiny-keccak`.

## Supply scan

Tools were installed inside the Linux builder, not on the host: `cargo install cargo-audit --version 0.22.2 --locked` and `cargo install cargo-deny --version 0.20.2 --locked`. Printed versions match. Publish times from the approved pins: cargo-audit `2026-06-05T12:33:04Z` (elapsed 10285020 seconds), cargo-deny `2026-07-09T17:58:09Z` (elapsed 7327915 seconds). Both are older than 604800 seconds at the adoption instant. `licenses/tool-age.tsv` records that arithmetic.

`cargo audit --json` exits 0. `vulnerabilities.found` is false and `count` is 0. The database timestamp in the JSON is `2026-10-02T10:58:33+02:00`.

`cargo deny --config ci/deny.toml --locked check` exits 4:

```
advisories ok, bans ok, licenses FAILED, sources ok
```

`ci/deny.toml` allows Apache-2.0, MIT, ISC, BSD-3-Clause, Unicode-3.0, Zlib, Unlicense, and MIT-0. `[graph]` sets `all-features = false` and `no-default-features = false`, and it sets no targets. This review did not add an allow, an ignore, a feature change, or a target list.

First rejection: `tiny-keccak` 2.0.2, `license = "CC0-1.0"`, not explicitly allowed. The deny graph reaches it from `arrow-array` through `ahash` / `compile-time-rng` / `const-random` / `const-random-macro`. The full diagnostic is `raw/phases/deny.err`.

Second rejection: `webpki-root-certs` 1.0.9, `license = "CDLA-Permissive-2.0"`, through `rustls-platform-verifier` and `oo-sdk`. The package is in the parent lock. `oo-sdk` was not part of this commit. This failure is inherited. A CC0-only fix would leave the same workspace command failing.

`bans ok` includes `warning[duplicate]` for `num-bigint` 0.4.8 and 0.5.1. `multiple-versions` is `warn`, so the warning does not by itself make the exit non-zero. Both versions are compiled into `oo-connectors` on the host, as the normal tree shows. 0.5.1 is the new one, via `arrow-buffer`. Other duplicate warnings (`base64`, `block-buffer`, `cpufeatures`, `crypto-common`, `digest`, `getrandom`, `hashbrown`, `r-efi`, `sha2`, `syn`, `synstructure`, `windows-sys`) name crates that this commit did not add.

## License bodies and age

Crate tarballs came from `https://static.crates.io`. Each file sha256 matches the lock checksum (`licenses/provenance-version.tsv`, column `checksum_match=yes`). Age uses `version.created_at` from the crates.io version object, compared with `2026-10-02T13:30:04Z`. The floor is 604800 seconds. Every row is `age_168h=yes` and `yanked=no`. The youngest new crate is `arrow-array` 60.0.0, published `2026-09-15T15:30:02.038482Z`, elapsed 1461601 seconds. `licenses/provenance.tsv` is the first parse. It took the first `created_at` in the file, which for most crates is not `version.created_at`. Do not use that file for age.

Nested `Cargo.toml` `rust-version` values, all at or below 1.94: arrow crates `1.88`, `half` `1.81`, `libm` `1.63`, `iana-time-zone` `1.62.0`, both `num-bigint` versions `1.60`, `windows-core` / `windows-result` / `windows-strings` `1.82`, `windows-implement` / `windows-interface` `1.74`. `android_system_properties`, `const-random`, `const-random-macro`, `crunchy`, `iana-time-zone-haiku`, and `tiny-keccak` do not set `rust-version` in the extracted manifest. The API `rust_version` field agrees where it is present.

Obligations read from the extracted bodies:

- `arrow-array` is `Apache-2.0 AND MIT`. Both apply. `LICENSE.txt` is the Apache 2.0 text. `LICENSE-MIT` is the MIT grant for chronoutil (Oliver Margetts). `NOTICE.txt` must be kept with the Apache distribution and names that MIT inclusion plus compact-thrift under Apache 2.0.
- `arrow-buffer`, `arrow-data`, and `arrow-schema` are `Apache-2.0`. Each ships `LICENSE.txt` and a `NOTICE.txt` that still carries the chronoutil MIT attribution. The NOTICE stays with the Apache license.
- `MIT OR Apache-2.0` crates ship both bodies: `android_system_properties`, `const-random`, `const-random-macro`, `half`, `iana-time-zone`, `iana-time-zone-haiku`, both `num-bigint` versions, and the five `windows-*` crates. One side satisfies the OR. The MIT texts include the copyright and the notice-preservation condition. The `windows-*` Apache files are the full Apache 2.0 text. `android_system_properties` `LICENSE-APACHE` is the short boilerplate that points at `http://www.apache.org/licenses/LICENSE-2.0` (554 bytes). Selecting the MIT side meets that OR.
- `crunchy` and `libm` are MIT. `libm` `LICENSE.txt` also retains musl, CORE-MATH, and the Sun math copyright notices, and says those portions are under terms compatible with that MIT license. The notices stay in the file.
- `tiny-keccak` `LICENSE` is the CC0 1.0 Universal waiver. That identifier is outside the allow list. The body being present does not make the deny check pass.

## What this review does not sign

Parent Reader, Loader, full SP03-W04, AT-SRC-03, AT-SRC-05, the vertical slice, publish, query, CLI, amd64, customer delivery, and a supply or product release. Source admission `34fae5ddd73ff250fde6f51aa681fb0f7fbd0362` and the a097 evidence stay inputs with their existing status. `oo.source.pgsql/v1` catalog numeric inspection was not retuned. Conversion remains `oo.source.pgsql.types/v1`.

## Cleanup

Own containers, volumes, network, and `/tmp/oo-a110-*` were removed after the phase files, the PostgreSQL log, the stats, and the hashes were in this directory. No global prune. Author containers and evidence were left. The evidence snapshot remains modified by the probe copy described above.
