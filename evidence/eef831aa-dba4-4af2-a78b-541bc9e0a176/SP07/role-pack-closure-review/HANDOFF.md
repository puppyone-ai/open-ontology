# Handoff

Independent closure review of the role Knowledge Pack compiler on `eff1959bf028a65b20b7fca855d79c23e4b188fd`, tree `2f08d4ecef7ad2f3b617e8a04afcb45f0cf0186f`, parent `9e3f757fb34746c64c6fca3d60cf0c5e342a1885`.

Verdict: **APPROVE LIMITED** the bounded role-pack compiler library slice. The requirement record is `REPORT.md`. The line-level notes are `FINDINGS.md`. The oracle description is `ORACLE.md`. Author evidence `9f357020` is not this proof. No product file was edited. The development ledger was not edited. Nothing was pushed.

The deciding oracle is `linux/probe-2/out/outcome.json`. Clerk legal SQL is `linux/probe-2/out/clerk/files/metrics.md` lines 34, 49, and 79, and `examples/verified.sql` lines 9 and 14. Pre-overlay smoke is `linux/smoke/`. The failed harness run is `linux/probe/`: the absence list contained `most_common`, which the row-security sentence uses. Probe and probe-2 share clerk digest `bb952bc8d8144473889f91525521cf186ea3fef164b1e5688dcba5aba8ab7831`. Do not read probe as a product leak.

No required change remains for the findings this review closed:

- `''` is decoded in `decode_supported_sql_strings` (`crates/oo-builder/src/role_pack.rs` lines 1400–1462) before `copies_hidden_label` (lines 1349–1352). `SELECT 'it''s visible' AS label_ok FROM wharf.crate` stays.
- Unquoted `DISTINCT`, including `SELECT DISTINCT FROM wharf.crate`, is rejected because `distinct` is in `SQL_KEYWORDS` (lines 1649–1651) and `parse_name` (lines 1832–1839) rejects keywords. `SELECT mass_kg AS "DISTINCT" FROM wharf.crate` stays.
- Relationship and entity names go through `visible_prose` (`select` lines 1014–1016, `entity_fully_visible` lines 1227–1230). `vault_bin` and `entities/vault_bin.md` are omitted. `example_kept` (lines 1189–1191) omits `rel:vault_bin`.

a168 F1, F2, and F3 are closed on the fresh fixture. Details are in `FINDINGS.md`.

REQ-OOS-05 and `docs/06-ontology-layer.md` section 7.1 are the measure. Cache separation, the independent Rust hash, sample refusal, RLS proof refusal, and the manifest’s unsupported labels held. Domains, tokenizer, read API, search, mount, authentication, the policy adapter, and publication gates G1–G13 stay open. This is not SP07-W06 approval. A visibility snapshot is not authentication and not a grant. A successful parse is not SQL execution.

The overlay under `overlay/` was applied only to a copy of the git archive. `Cargo.lock` was not edited. Slot B and PostgreSQL were not used.

`CLEANUP.txt` records removal of this task’s network, cargo volume, target volume, and `/tmp` tree. The builder containers were removed by their launch scripts. The pinned Rust image was left in place. `docker prune` was not run. `MANIFEST.sha256` is the SHA-256 of every file in this directory except itself. `source/blobs.sha256` is the fixed-commit content hash and git blob id.
