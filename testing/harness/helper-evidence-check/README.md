# helper-evidence-check

Standalone checker for the five containment-supervisor registry records. It is not a member of the product workspace. It uses the Rust standard library only.

`audited`, `admission`, `fresh_current_scan`, and `release_signoff` stay false. A successful run prints `records=5 audited=false`. That line accepts the unaudited bundle. It is not a scan, a legal approval, or a release signoff.

## Reviewed baseline

The adoption anchor is compiled into `src/trust.rs`. It is not read from the inventory, the records, or a live git command.

| Anchor | Value |
| --- | --- |
| Helper-lock commit | `f1a6005971e3dc3075094c87ed670fc9d25e9ae0` |
| Author and committer time | `2026-10-02T19:10:37+08:00` |
| Adoption instant | `2026-10-02T11:10:37Z` |
| Helper lock sha256 | `14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1` |

Those git facts were checked before the constants were kept. A later baseline needs a reviewed change to this crate and to its evidence. Agreement between an inventory and a record does not move the anchor.

Age is the completed UTC days from that version's `version.created_at` to the adoption instant. The stored day count has to match the recomputation. The publication input is the age file's `version_created_at`, and it has to match the version JSON `created_at`.

The reviewed choice is MIT for bitflags, cfg-if, and libc. nix and cfg_aliases stay a null choice. An OR expression still needs both upstream bodies and both obligation rows. A paired Apache-2.0 choice is rejected for this baseline even when the license bytes match. That is the reviewed choice for these crates. It does not say Apache-2.0 is forbidden on another baseline.

## Retained audit

The checker reads the retained a101 `scan-audit.out`. That object has no `status` member. The checker does not add one. A present `status` fails closed: `unverified` and any other non-blank string are a fake claim, and a non-string or blank status is malformed. Required fields from that file are `database.last-commit`, `database.last-updated`, `database.advisory-count`, `lockfile.dependency-count`, `settings`, `warnings`, and `vulnerabilities` (`found`, `count`, `list`). Missing ones fail closed. Updating the inventory digest to the forged file does not bypass these anchors. `fresh_current_scan` stays false. This crate does not rerun cargo-audit or cargo-deny.

## JSON depth

`MAX_DEPTH` is 64. Depth counts containers only. Parsing starts at depth 0, before a value is read.

- A scalar root stays at depth 0.
- An object or array root is depth 1. A container inside a container adds one. A scalar does not.
- The depth check runs before the nested container function is called.
- 64 nested containers is the deepest accepted value. 65, and a 20000-deep object, array, or mixed value, returns `code=bound` exit 22. The process does not panic, abort, or raise a signal.

Array length and object key count have their own limits. Malformed escapes, invalid UTF-8, and duplicate keys still fail closed.

## Not signed by this checker

Unknown record keys, hard links, helper-manifest digest binding, pre-stat time-of-check/time-of-use, the hash subprocess timeout, and the hash child's environment are outside this fix. Hostile-input behavior past the bounds above is not claimed. AT-SEC-05, the whole graph, owners, platform libc, runsc, CI, secrets, customer, and release stay unsigned.
