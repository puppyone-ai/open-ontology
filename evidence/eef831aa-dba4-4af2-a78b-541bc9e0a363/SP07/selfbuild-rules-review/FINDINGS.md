# Findings

Decision: APPROVE_LIMITED for frozen commit `e5427c1a18e2ddbb480791d214c76888eb7ca998` only. SP07 stays open. No new product unit was added.

## Preserved smoke

`linux/smoke/docker-run.exit` is 1. `linux/smoke/smoke.exit` is 1. `linux/smoke/phases/toolchain.exit` is 0 (`rustc 1.94.0`, Linux arm64). `linux/smoke/clippy-component.txt` is `absent` and `linux/smoke/phases/clippy-component.exit` is 1. The pinned Rust image does not ship Clippy. That smoke exit is an image baseline, and it is not a pass. The product crate was not mounted in the smoke container. The container `oo-a363-rules-smoke` was removed. The log was not overwritten.

## Clippy in the proof builder

`linux/proof/clippy-component.log` is one line: `info: downloading component clippy`. `linux/proof/clippy-component.txt` is `clippy-aarch64-unknown-linux-gnu`. `linux/proof/phases/clippy-component.exit` is 0. `linux/proof/phases/clippy.exit` is 0 for `cargo clippy --locked --offline --all-targets -- -D warnings`. The install used the discarded container `HOME=/tmp` rustup directory, so a later container does not still have the component. The proof record is the evidence. `linux/oracle-2` did not install Clippy and did not rerun Clippy. `linux/oracle-2/clippy-not-rerun.txt` records that skip.

## Proof phases that passed

These exits in `linux/proof/phases/` are 0 and were not rerun:

| Phase | Exit |
|---|---|
| toolchain | 0 |
| clippy-component | 0 |
| fetch | 0 |
| test | 0 |
| clippy | 0 |
| lock-subset-build | 0 |
| lock-subset | 0 |
| cli | 0 |
| forbidden | 0 |

`linux/proof/test.log`: lib 4 passed, api 12 passed, cli 6 passed, plus two empty harness results, 0 failed. Lock subset summary: `registry_triples=11 missing=0 mismatch=0 direct_pins_present=true`. The eleven registry triples are itoa 1.0.18, memchr 2.8.3, proc-macro2 1.0.107, quote 1.0.47, serde 1.0.229, serde_core 1.0.229, serde_derive 1.0.229, serde_json 1.0.151, syn 3.0.6, unicode-ident 1.0.26, and zmij 1.0.23. `cargo-update-invoked.txt` is `no`. The checker `tools/lock_subset.rs` is byte-identical to the a354 copy (sha256 `dcf85a75065a6c1b734721ee3cac56c4f9638a95b20ed1b50719dc97e70f6a13`).

Sealed CLI `linux/proof/shop-review.json` sha256 `f96708218cb1420bbe4a5e27cc7ef26d7f0a574fe3473d14bb926689781fc2ce`. Stderr was empty. Forbidden counts are 0 for `甲注评名录`, `e901f90576b625d9f2252ed2f8659f90d792002742b9d37348549a16c867c991`, `Ignore previous`, `non-PII`, and `current_privileges`.

`linux/proof/proof.exit` and `linux/proof/docker-run.exit` stay 1 because the script stops on the first failing phase. That failure is the review harness below, after the product phases had already exited 0. Host config: oom false, memory 6442450944, cpu 1000000000, privileged false, pids 4096. Container `oo-a363-rules-proof` was removed.

## Failed review harness, then the oracle retry

`linux/proof/phases/oracle-build.exit` is 1. `linux/proof/oracle-build.log` records three rustc errors in the first `oracle/main.rs`: missing lifetimes on `rows` and `ids` (E0106), and a closure parameter that needed a type (E0282). The product sources were not edited. The oracle gained explicit lifetimes, an explicit `|id: &&str|`, and `Value` comparisons that use `json!(0)` and `as_str()`. Output paths moved to `linux/oracle-2/` so the failed proof log stays intact.

`linux/oracle-2/docker-run.exit` is 0. `oracle.exit` is 0. `oracle.log` is `oracle_ok`. `oracle.stderr` is empty. `phases/oracle-build.exit`, `phases/oracle.exit`, and `phases/cli-oracle-bytes.exit` are 0. Host config: oom false, exit 0, memory 6442450944, cpu 1000000000, privileged false, network `oo-a363-eef831aa-net`, pids 4096. CapDrop is ALL. Container `oo-a363-rules-oracle2` was removed.

`oracle-low.json` sha256 `f80ea769dd161ea27b03dc4fde02f6e4d8858eecb1a8e1e9044258cdd6b42995`. `oracle-shop.json` sha256 `3a120eb8aba359cfeb06d20caa1dda6f93163cbe8ea4d288897031ce7fece05d`. That shop file plus one newline matches the preserved CLI bytes.

## Author fixture 5 to 40

The a354 packet, which this review did not rerun, records attempt 3: `n_distinct` 5 on the bridge foreign keys made the producer emit 6 enums and 22 experts against a gold file of 4 enums and 20 experts. The author fixture then used 40. `d-truth.json` was not regenerated.

Checked here against the frozen tree:

- Gold sha256 is still `2f3adbeeb3037b547d33eb611d043e9ee11b52e32b4983fd3b9148575f928f66`.
- Gold enums are exactly the four demo ids. No gold enum id contains `bridge`.
- Gold `col/demo.bridge.left_id#1` and `col/demo.bridge.right_id#1` are `foreign_key`, `none`, `auto_accepted`.
- `tests/common/mod.rs` contains `nd: Some("40")` for `left_id` and `right_id` and does not contain `Some("5")`.

`40` is a legitimate non-low, non-unique statistic for the `many_to_one` rows the gold already states. It is also a narrowing of author coverage: the contract enum rule does not exempt an integer declared foreign key, so `n_distinct` 5 must still emit an undetermined unapproved enum while the semantic type stays `foreign_key` and auto-accepted. The independent low-card oracle passed that combined result. The gold file was not edited to match producer output.

## Clauses the oracle accepted

- Personal and sensitive columns stay pending and are never auto-accepted. Unknown `pii` stays pending.
- Auto-accept is limited to the contract list: `boolean_flag`, `money`, `quantity`, `ratio`, `event_time`, `date`, `audit_time`, `identifier`, `foreign_key`, `status_code`, `code`, and only with `rule_high_confidence`, `pii` `none`, confidence `1`, and uncertainty `0`.
- `auto_accept` `none` forces every column pending and does not authorize grants.
- Empty profile tables are zero profile. Card `n_distinct` is ignored. `zt` does not become `status_code` without a profile.
- Comments and `current_privileges` are absent from the canonical review.
- Two naming parents stay in the relation list with `conflict` true and `selected` null. No entity is merged.
- Identity, input bound, schema token, sample policy, and visible profile values refuse with the contract codes and no document.
- Shop `label` and `memo` negative fractions are above the low-cardinality range. `qty` with `n_distinct` `-1` stays `quantity` and is not an enum.

## Inputs this review did not use

No raw sample file, no live source database, no authentication store, no model call, and no PostgreSQL. The sealed catalog, profile, and cards were mounted read-only. The crate was mounted read-only.
