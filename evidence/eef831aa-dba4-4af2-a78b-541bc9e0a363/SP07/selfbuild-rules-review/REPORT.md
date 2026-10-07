# Frozen E49 selfbuild-rules review

Independent review for UUID `eef831aa-dba4-4af2-a78b-541bc9e0a363`. Reviewer is one Grok 4.7 session. Product tree `tools/selfbuild-rules` is the frozen commit `e5427c1a18e2ddbb480791d214c76888eb7ca998` and was left unchanged. This packet accepts that frozen unit only.

Decision: APPROVE_LIMITED

The limit is the frozen contract `oo.selfbuild-rules/v1`: review-only S2–S4 cards. SP07 stays open. No new product unit was added. Parent and goal acceptance stay open. Raw samples, a live source, authentication, a model, and PostgreSQL were not required and were not started.

## What was reviewed

Contract `tools/selfbuild-rules/CONTRACT.md`. Docs `docs/06-ontology-layer.md` sections 4.4 S2, 4.5 S3, 4.6 S4, and 4.10. Spec rows AT-OOS-02 and AT-OOS-03 in `specs/open-ontology-v0.1-spec.md`. Author packet `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a354/SP07/selfbuild-rules/`. Sealed shop inputs are the three pass2 files under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/live/pass2/`.

Source pins on every review: catalog `149944271ef1c2cb555954d65248413df2166f1c`, cards `c23db28dc27f5cf20741dd36e8a8e88129ad1715`. Output protocol `oo.selfbuild-rules.review-cards/v1`. `effect` is `review_only`. `grants` is `not_authorized`. `authenticated` is false. `comment_disclosure` is `withheld`. `entity_merge`, `metric_certification`, and `knowledge_graph` stay unperformed or unemitted. `publication` is `unavailable`.

`source-product.sha256` matches the 14 frozen files, including gold `tests/fixtures/d-truth.json` at `2f3adbeeb3037b547d33eb611d043e9ee11b52e32b4983fd3b9148575f928f66` (git blob `c475773ca5c2e0dc1aa14cbae71474abf1e644cd`).

## Linux evidence

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux arm64, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Each builder had 6 GiB, 1 CPU, pids 4096, `--pull never`, `--cap-drop ALL`, `no-new-privileges`, user `0:0`, private network `oo-a363-eef831aa-net`, and volumes `oo-a363-eef831aa-cargo` and `oo-a363-eef831aa-target`. Host cargo count was 0. `cargo update` was not invoked.

| Run | docker-run.exit | What the exit means |
|---|---|---|
| `linux/smoke` | 1 | Image baseline. Toolchain exit 0. Clippy component absent. Preserved. This exit is not a pass. |
| `linux/proof` | 1 | Product phases below exited 0. The run then stopped on the first oracle compile. |
| `linux/oracle-2` | 0 | Oracle-only retry. OOMKilled false. Container removed. |

Proof phases that exited 0, and that were not rerun: toolchain, clippy component install (`clippy-aarch64-unknown-linux-gnu`, log line `info: downloading component clippy`), `cargo fetch --locked`, `cargo test --locked --offline` (lib 4, api 12, cli 6, 0 failed), `cargo clippy --locked --offline --all-targets -- -D warnings`, lock-subset build and run (`registry_triples=11 missing=0 mismatch=0 direct_pins_present=true`), sealed shop CLI, forbidden substring counts. `linux/proof/phases/oracle-build.exit` is 1 and stays 1.

`linux/oracle-2` reused those rlibs (`libselfbuild_rules-394c53159477c4a3.rlib`, `libserde_json-824f03b7bda2249d.rlib`). Phases toolchain, rlib, oracle-build, oracle, and cli-oracle-bytes all exited 0. `oracle.log` is `oracle_ok`.

## Independent oracle

`oracle/main.rs` carries hand literals. It does not load producer output as gold. `seal_card` stamps input card hashes only. The scorer does not compare uncertainty, so the oracle compares uncertainty strings itself.

Low-card bridge, `n_distinct` `"5"` on `hand.bridge.left_id` and `hand.bridge.right_id`: 4 columns, both foreign keys `foreign_key` / `none` / `auto_accepted` / uncertainty `0`, two declared `many_to_one` relations at uncertainty `0` and confidence `1`, one inferred `many_to_many` at uncertainty `0.5` and confidence `0.5` (`rel/hand.bridge/hand.left.id/to/hand.right.id/inferred`), two enum cards `undetermined` / approved false / values `withheld`, exactly 3 expert cards, leak 0, score matched, canonical bytes stable. `auto_accept` `none` on the same inputs leaves all 4 columns `pending`, keeps the 2 enums, and yields 7 expert cards. `grants` stays `not_authorized`.

Zero profile ignores a forced card `n_distinct` of `"3"`: `z.item.id` is `identifier` and auto-accepted; `national_id` is sensitive pending uncertainty `1`; `phone` is personal pending uncertainty `0`; `zt` and `mystery` stay `unknown` / `unknown` / pending. No enum is emitted. `RAWNOTE` is absent.

Weak naming conflict on `w.audit.user_id`: column `unknown` / `none` / pending uncertainty `0.5`; two `naming` `many_to_one` relations with `conflict` true, uncertainty `0.5`, confidence `0.5`; `conflict/w.audit.user_id#1` has `selected` null. `entity_merge` stays `not_performed`.

Refusals, with no review document: cards schema hash of `c` repeated is `identity.stale`; a 1,048,577-byte catalog is `bound.input`; token `owner_grant` is `schema`; `sample_policy` `raw` is `profile.samples`; resealed `most_common_vals` `visible-values` is `profile.values`.

Sealed shop: 14 columns, 2 declared `many_to_one` relations, 0 enums, 5 expert cards, schema hash `eae55fc9a122759d06204b86b0147a1ef9a1066470156039362d7dd52d851b74`, evidence hash `2c66f95d485a9df43d08edd0c8f097c7a283ed15b4511735b6482438a764369b`. Personal and sensitive columns stay pending. `label` (`-0.94725` of 8000) and `memo` (`-0.9206333` of 90000) stay unknown and pending. `qty` stays `quantity` and auto-accepted. Auto-accepted semantics stay inside the contract allow-list. Canonical `oracle-shop.json` plus one newline equals `linux/proof/shop-review.json` (`f96708218cb1420bbe4a5e27cc7ef26d7f0a574fe3473d14bb926689781fc2ce`). Forbidden counts in that CLI file are 0 for `甲注评名录`, the party comment sha256 `e901f90576b625d9f2252ed2f8659f90d792002742b9d37348549a16c867c991`, `Ignore previous`, `non-PII`, and `current_privileges`.

## Fixture judgment

Gold `d-truth.json` is unchanged: exactly 4 enum ids (`enum/demo.order.zt#5`, `enum/demo.sys_dict.code#2`, `enum/demo.sys_dict.kind#1`, `enum/demo.sys_dict.label#3`), and both `demo.bridge` foreign-key columns remain `foreign_key` / `none` / `auto_accepted`. The author fixture in `tests/common/mod.rs` now has `n_distinct` `Some("40")` for `left_id` and `right_id`, and it does not contain `Some("5")`.

`40` is outside 1..=32 and is not the unique sentinel `-1`, so those declared foreign keys stay `many_to_one`. That input matches the cardinality the unchanged gold already records.

The same edit drops the author suite's low-cardinality declared-FK enum case. The contract still requires an undetermined, unapproved enum for a non-personal integer column whose `n_distinct` is 5, while the column semantic stays `foreign_key` and auto-accepted and the bridge stays `many_to_many` with `entity_merge` `not_performed`. The independent oracle is that case, and it passed. The author gold was not rewritten.

## Boundaries

Owned path is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a363/SP07/selfbuild-rules-review/**`. The crate, root manifests, sealed shop files, and the a354 packet were not modified. Frontend sessions a351, a353, and a352 were not touched. The frozen contract emits no containment statistic and no join path; this limit does not accept the rest of S2–S4.
