# a254 reviewed integer quantity

Child extension of the reviewed-policy adapter. Contract `oo.gateway.reviewed-policy-quantity/v1` is `contracts/gateway/reviewed-policy-quantity-v1.md`, commit `6c426aa5839fbebaf67b84d07cfd82b8e01391af`, frozen before the product commit. The compatibility change is commit `83b4f94a12c527ff8fd77eadd2909437e63d908c`. The CLI fixture is commit `e447249ab1a7f4c82df4729090b1285c9ef4a4ec`.

Fixed archive: `c828e2781fc37eb62249fab54fa0b7fff86f0249`. Pre-edit builds used `git archive` of that commit. The working tree was not the compiler input for the baseline.

`live/outcome.json` records `parent_acceptance` false, `component_approval` false, `development_unit` false, `m1_i01` `not_accepted`, and `m1_i02` `not_accepted`. This directory does not accept a parent plan item, does not close the adapter, and does not release.

## Owned paths

- `crates/oo-gateway/src/reviewed_policy.rs` — `compatible()` integer arm only
- `crates/oo-gateway/tests/reviewed_policy_quantity.rs` — new
- `contracts/gateway/reviewed-policy-quantity-v1.md` — already committed as `6c426aa5`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a254/SP06/reviewed-policy-quantity/**`

`git diff c828e2781fc37eb62249fab54fa0b7fff86f0249 -- crates/oo-gateway contracts/gateway Cargo.toml Cargo.lock` after the two product commits is the new contract plus three lines in `compatible()`. `lib.rs`, `policy.rs`, the reviewed-policy binary, the existing gateway tests, manifests, and `Cargo.lock` were not edited. a207 remains the connector manifest and root-lock writer. Post-edit `linux/postedit/lock-hash.txt` is `unchanged 16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

Git blob of `reviewed_policy.rs` at the fixed commit: `56b62fdc1ab9864b99d992ccb46782fc34c1a507`. Blob at `83b4f94a`: `95e68417e6fe1be024123e3aa2b7c2301402eb21`. Test blob at `e447249`: `b31773e9cb61d73c88b217225aa1d5b6de140fa9`. `semantic_name` already mapped `SemanticType::Quantity` to `"quantity"` on the fixed blob. The new arm is the only product change.

## Historical failure this child continues

a248 `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a248/SP11/pg-operator-composition/` stopped at `reviewed-policy-clerk` exit 11, stderr `reviewed-policy: semantic`. `shop.order_doc.amount` was confirmed `quantity` on base `int8`. That run produced no policy DTO, no install, no analyze, and no query. Its evidence was not edited. Its limited acceptance stays historical. This child is the same adapter cell, not a new component.

## What the code does

`admit_one` still resolves the column, loads the decision, and calls `check_decision` and `check_output` before `column_spec`. `column_spec` still requires a data type and `physical_type` before `compatible`. Missing, stale schema, stale fact, rejected, unbound, unknown, and mismatch still return `decision`. A hidden column still returns `seal`.

`compatible` now accepts `SemanticType::Quantity` together with identifier, foreign key, code, and status code when the physical base type is `int2`, `int4`, or `int8`. `bool` still accepts only `boolean_flag`. The text list is unchanged. `float4`, `float8`, `numeric`, and `date` still fail the base-type check. Money, ratio, event time, date, audit time, geo, json, and system are not added. Core `validate_dataset` requires `unit` only for money, so quantity still has no unit constraint.

## Toolchain

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `--pull never`. `rustc 1.94.0 (4a4ef493e 2026-03-02)` and `cargo 1.94.0 (85eff7c80 2026-01-15)` on the successful smoke console. Each new builder was 6442450944 bytes, `NanoCpus` 1000000000, `CapDrop` `["ALL"]`, `Privileged` false, network `oo-a254-eef831aa-net`, `OOMKilled` false. Private PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. One database, name `oo-a254-pg`, alias `oo-a254-pg`, SAN `DNS:oo-a254-pg`. `pg/up.exit` is 0.

Recipe origins and the attempt-1 gate note are in `origin/recipes.txt`.

## Runs

| Run | Top exit | Docker exit | Container |
|---|---|---|---|
| `linux/smoke-attempt-1` | 2 | 2 | `ae68e8c23a4687589dd22d489f883d8042e4d3c6fc66c1373988d472714bdbad` |
| `linux/smoke` | 0 | 0 | `90198a327a5be2fb25a8879af48cdcc936d42c72dc80ac15cb89b89abb1e1afc` |
| `linux/postedit-attempt-1` | 101 | 101 | `4aca12106d85311e165a9fe204534a67c6cd3b398ca1b2cdfabb837712940fc5` |
| `linux/postedit-attempt-2` | 1 | 1 | `85c70bdb854b6050e37acb36c729bda3189bf1bc79c0bc30ab4180d27719402d` |
| `linux/postedit-attempt-3` | 101 | 101 | `3ad8c6c5fcb77e548f62aaf81398065aeb646ee71e95ce77e3b9b4da6b2fb1a4` |
| `linux/postedit` | 0 | 0 | `d142a3f4cdb961e4c585554a4452b191a4f6d07e38af14c89a88c9337a686ac8` |
| `linux/chain` | 0 | 0 | `1707f5f828c670c6953789693f0b8822ceb0ad0cf20d5beaa2e0d1ab990bfbd7` |

Smoke attempt 1 is preserved. Its gate searched the whole archived file for `SemanticType::Quantity`. That string is the existing `semantic_name` arm. The retry gate prints only the `compatible` integer arm, from `"int2" | "int4" | "int8" => matches!` through the next `"text" =>`, and the archive arm has no `Quantity`. Console ends `preedit_smoke=ok`. Boundary tests on that archive: `reviewed_policy` 21 passed in 1.30s, `reviewed_policy_cli` 6 passed in 0.43s.

Post-edit success, `linux/postedit/`: `reviewed_policy` 21 passed in 1.40s, `reviewed_policy_cli` 6 passed in 0.45s, `reviewed_policy_quantity` 6 passed in 0.55s. Clippy phase exit 0 (`cargo clippy --locked --offline -p oo-gateway --lib --bin reviewed-policy --test reviewed_policy_quantity -- -D warnings`). Console ends `postedit=ok`. The reviewed-policy binary hash recorded there is `2581ee8b29d923a3421f88e20d45cb06fb1cda18cbfea1d6cabe24c70e6cbd72`. The chain `bin-identity` phase is 0 against that hash. Role-pack-bundle was built from the unpatched archive with `--locked`. Harness `cargo build` and harness Clippy omitted `--locked` because the overlay adds a path member. The registry-triple lock diff phase is 0.

Attempt diagnoses are `linux/postedit-attempt-1/DIAGNOSIS.txt` (E0521, non-static `&str`), `linux/postedit-attempt-2/DIAGNOSIS.txt` (clippy component absent for 1.94.0), and `linux/postedit-attempt-3/DIAGNOSIS.txt` (`wrong_self_convention` on `to_policy`, `to_filter`, `to_spec`, `to_class`, renamed `into_*`). There is no failed chain attempt. The `--locked` harness lines were removed before the chain container started.

## Library and CLI fixture

`crates/oo-gateway/tests/reviewed_policy_quantity.rs` does not open a database. Binding generation is 7. The test asserts that generation is neither 48 nor 1.

Hand decimals, parsed as `i16`, `i32`, and `i64`:

| Column | Values |
|---|---|
| `amount_i2` | `-32768`, `32767`, `0` |
| `amount_i4` | `-2147483648`, `2147483647`, `0` |
| `amount_i8` | `-9223372036854775808`, `9223372036854775807`, `0` |

Positive path: compiled `captured-pg-draft` and `reviewed-effective-oos` keep `semantic_type` `quantity` on those three columns. `reviewed-policy` exits 0. The DTO keeps the names, confirmed status, quantity on the amount columns, identifier on `batch_i8`, `row_filter` absent, `pii` none on `amount_i2` and `amount_i4`, and declared explicit allow on personal `amount_i8`. `compile_column_acl_set` accepts that DTO. A second CLI run matches the first stdout.

Refusals on the same binaries: wrong explicit allow exits 12 `reviewed-policy: pii_allow`; quantity on `float4`, `float8`, `numeric`, `date`, `bool`, and `text` exits 11 `reviewed-policy: semantic`; missing decision, schema hash `ef` repeated, and fact fingerprint `ab` repeated exit 10 with `missing`, `stale_schema`, and `stale_fact`. Money on `int8` fails in `reviewed-effective-oos` with `oos.invalid` and `effective OOS validation failed`, empty stdout. Ratio, event time, date, audit time, geo, json, and system on `int8` exit 11. Numeric catalog rows use `unbounded_or_wide_numeric`. The other wrong physical types keep `known_builtin_not_converted`.

## Live composition

`linux/chain/chain.exit` is 0. Console ends `quantity_chain=ok`. Import result and outcome: source generation 48, target generation 1, run id `13a8517a-dc78-48b2-af65-60e8ac10d8d7`, state `awaiting_publish`. `src/pg.rs` `compare_import` reads `source_generation` and `target_generation` from `_oo.full_import_run` for that run id, reads `generation` from `_oo.target_generations` where `shard_id` is `a254main` and `state` is `adopted`, and stops if the sealed pair disagrees with the import JSON, if the fence disagrees with the sealed target, or if either target value equals the source cut. Mapping `run_generation` must equal the source cut. `src/policies.rs` stops when policy generation equals the target generation, then requires policy generation to equal the mapping run generation.

Clerk reviewed-policy stdout is exit 0 and contains `generation` 48 and `amount` with `semantic_type` `quantity` and pii class `none`. The install request pastes that DTO. `install-column-acl-clerk.exit` and `install-column-acl-peek.exit` are 0. `provision_clerk` is `installed`. `provision_peek` is `replay`. `analyze-pg.exit` is 0. The analyze request carries `source_generation` 48. Statistics keep that field. The target fence is not written into it.

`live/steps/query-clerk-order_doc.stdout` and the star twin return three rows. The amount cells are int `-9223372036854775808`, `9223372036854775807`, and `0`. Peek's order projection is order id, customer id, and the posted flag. Amount is on the clerk document, which is the query that carries it.

Expected non-zero steps inside the harness, also exit 0 for the chain as a whole: `import-pg-not-ready` 1, `query-insert` 6, `query-pos` 6, `query-before-analyze` 5, `query-hidden` 6, `role-pack-bundle-clerk-again` 1, `role-pack-private-note` 1, `role-pack-wrapped-draft` 1, `role-pack-review-stdout` 1.

## Next seams

Left open on purpose. Each one needs its own frozen contract before code.

- Money with the core unit constraint.
- Ratio, event time, date, audit time, geo, json, and system.
- Float4, float8, numeric, and date physical codecs.
- Quantity on text or bool.
- Using the target fence generation as the policy generation. The existing installer still rejects that.

Authentication, authorization, publication, an audit WAL, and live row-level security stay open. `outcome.json` records those as `not_claimed` or `missing`.

## Inventory and cleanup

`tools/sha256.rs` is the std-only oracle copied from a248, git hash-object `b69a2af13099ce4baf2d04e79c5692d9988ab67d`. `SHA256SUMS` is `sha256 dir` over this directory. The tool skips paths ending in `/SHA256SUMS` and `/SHA256SUMS.sha256`, then writes `SHA256SUMS.sha256`. Host `shasum` is not this inventory.

`cleanup/run.log` removes container `oo-a254-pg`, volumes `oo-a254-eef831aa-cargo`, `oo-a254-eef831aa-pgdata`, and `oo-a254-eef831aa-target`, network `oo-a254-eef831aa-net`, and `/tmp/oo-a254-eef831aa-dba4-4af2-a78b-541bc9e0a254`. The pinned images stay. No prune.
