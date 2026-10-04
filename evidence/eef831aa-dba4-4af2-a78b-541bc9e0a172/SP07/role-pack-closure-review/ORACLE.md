# Oracle

The independent check is Rust. `overlay/role_pack_a172_review.rs` is `#![forbid(unsafe_code)]`. Digests use `sha2` 0.10.9 `Sha256` and a hex encoder in that file. Canonical JSON is written in that file: sorted object keys, no extra spaces, the short escapes the library uses. The test does not call `oo_core::api::payload_sha256` or `oo_builder`'s hash helper. `sha2` is already an `oo-builder` dev-dependency. `Cargo.lock` was not changed.

No Python file was added. The a168 script `oracle.py` was not run.

The test calls `oo_builder::compile_role_pack` on documents built in the test. It dumps every pack file under `OO_A172_OUT` before the assertions, then writes `outcome.json`. The deciding dump is `linux/probe-3/out/`.

The fixture is fresh. It does not reuse the author strings `queue_note`, `客户备注`, `yard.slip`, or `é标记`. Hidden column `tick_col` has label `签'记`. Hidden column `quote_col` has label `签"记`. Hidden column `mark_nfd` has the NFD label `e\u{0301}货签`. Hidden column `quarter_start` is also a declared anchor, so a phrase whose start is `quarter_start` is a legal anchor expression and a hidden-column reference. Phrase label `hold_bin` uses the legal anchors `month_start` and `today`.

Certified metrics carry `owner`, `default_anchor`, one test, and `control_total`, because `validate_metrics` requires those fields. The default control SQL is `SELECT count(*) AS n FROM dock.scale`. `kept_mass` and `ctrl_leak` replace that control. The enum value with status `unexplained` uses class `unexplained`. The first probe used class `open` for that status and the document was refused at `/values/1/class` before closure. That run is `linux/probe/`.

Hash check rebuilds the cache key from the principal and role passed by the caller. It does not parse the key. Refusal checks use a fresh document for each code. Path `../hold_bin` must come back as `path.invalid` at `/` and the display must not contain the rejected text.

`## distinct_bare` and the `hold_bin` relationship/entity compile are recorded in `outcome.json` and do not, by themselves, fail the assert. The assert fails when `closure_violations` is non-empty. On probe-3 that list has two entries, both the `签''记` / `## sq_leak` leak.

The visibility snapshot is an input to `compile_role_pack`. The oracle does not authenticate, does not grant, and does not execute SQL.
