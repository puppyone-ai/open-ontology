# Oracle

The independent check is Rust. `overlay/role_pack_a176_review.rs` is `#![forbid(unsafe_code)]` at line 7. Digests use `sha2` 0.10.9 `Sha256` and a hex encoder in that file. Canonical JSON is written in that file: sorted object keys, no extra spaces, the short escapes the library uses. The test does not call `oo_core::api::payload_sha256` or `oo_builder`'s hash helper. `sha2` 0.10.9 and `unicode-normalization` 0.1.25 are already `oo-builder` dev-dependencies. `Cargo.lock` was not changed.

No Python file was added. The a168 script `oracle.py` was not run and was not copied.

The test calls `oo_builder::compile_role_pack` on documents built in the test. It dumps every pack file under `OO_A176_OUT` before the assertions, then writes `outcome.json`. The deciding dump is `linux/probe-2/out/`. The overlay was copied onto a copy of the git archive. The archive itself stayed unmodified. The product worktree was not the compile source.

The fixture is fresh. It does not reuse the a168, a172, or a174 strings `queue_note`, `客户备注`, `yard.slip`, `é标记`, `dock.parcel`, `签'记`, `签"记`, `é货签`, `hold_bin`, or `tick_col`.

Datasets are `wharf.crate`, `wharf.scale`, and `pier.berth`. Hidden column `tick_sq` has label `舱'记`. Hidden column `tick_dq` has label `舱"记`. Hidden column `vault_bin` has label `暂存舱`. Hidden column `mark_nfd` has the NFD label `e\u{0301}舷签`. The test records that NFC form as `é舷签` and that the two forms differ. Hidden column `quarter_start` is also a declared anchor. A phrase whose start is `quarter_start` is a legal anchor expression and a hidden-column reference.

Certified metrics carry `owner`, `default_anchor`, one test, and `control_total`, because validation requires those fields. The enum value with status `unexplained` uses class `unexplained`.

The strings the closure has to judge:

- legal visible string `SELECT 'it''s visible' AS label_ok FROM wharf.crate`
- hidden-label string `SELECT '舱''记' AS mass_kg FROM wharf.crate`
- quoted hidden label `SELECT mass_kg AS "舱""记" FROM wharf.crate`
- kept doubled quotes `SELECT mass_kg AS "净""重" FROM "wharf" . "crate"`
- kept example `SELECT mass_kg AS "净重" FROM "wharf"."crate"`
- unquoted `SELECT DISTINCT mass_kg FROM wharf.crate`
- degenerate `SELECT DISTINCT FROM wharf.crate`
- lowercase `select distinct mass_kg from wharf.crate`
- quoted identifier `SELECT mass_kg AS "DISTINCT" FROM wharf.crate`
- quoted hidden table `SELECT mass_kg FROM "pier"."berth"`
- prose `本季到港只计可见列，净重以公斤计。` and glossary `it's a visible note`

`control_leak` displays the legal string and puts the hidden-label string in `control_total.sql`. Control SQL is scanned and is not copied. `unit_leak` sets the unit field to `vault_bin`. A relationship and an entity are named `vault_bin` with visible endpoints. Example `ex-rel-hidden` uses `rel:vault_bin` and otherwise legal SQL. Example `ex-hidden-quote` uses only visible `uses` entries, so omission depends on the quoted SQL scan.

Hash check rebuilds the cache key from the principal and role passed by the caller. It does not parse the key. Refusal checks use a fresh document for each code: stale policy, role `clerk`, `per_role` false, raw samples, missing scale proof, `proven: false`, proof left on a dataset with row security `none`, confirmed classification with no PII, pending classification, dataset `sample_values`, and `normalize_pack_path("../vault_bin")`. Path rejection must be `path.invalid` at `/` and the display must not contain `vault_bin`.

`linux/probe/` is the same fixture with one assertion difference: the absence list contained the token `most_common`. That token is in the required row-security sentence. The deciding oracle asserts `most_common_vals` instead. Probe and probe-2 clerk digests are the same.

The visibility snapshot is an input to `compile_role_pack`. The oracle does not authenticate, does not grant, and does not execute SQL.
