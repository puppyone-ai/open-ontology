# Handoff

Independent closure review of the role Knowledge Pack compiler on `ef34ad7bc6d89376d0b1952dfe49e5efb2a8744d`, tree `9f2f6d353a957cc400c7be01d575599260117fe4`, parent `06a99f1181b648863975eb214c14f949d835a67e`.

Verdict: **REQUEST_CHANGES**. The requirement record is `REPORT.md`. The line-level notes are `FINDINGS.md`. The oracle description is `ORACLE.md`. Author evidence `89cc545d690f36794d6a6b4bb3356f5ea05a56a7` is not this proof. No product file was edited. The development ledger was not edited. Nothing was pushed.

The deciding oracle is `linux/probe-3/out/outcome.json` and `linux/probe-3/out/clerk/files/metrics.md` lines 69–81. Pre-overlay smoke is `linux/smoke/`. Failed harness runs are `linux/probe/` (OOS enum class before the closure assert; clippy component absent) and `linux/probe-2/` (the same product leak, plus clippy `collapsible_if` in the oracle). Do not read those as a second product result.

Required change, `crates/oo-builder/src/role_pack.rs`:

`label_in_text` (1378–1384) and `all_idents` / `take_ident` (1467–1551) compare the raw NFC source. `parse_string` (1775–1791) accepts `''` as one apostrophe, `verified_sql` (1604–1610) returns true, and `render_metrics` (2223–2226) copies the SQL. Hidden label `签'记` is published as `SELECT '签''记' AS mass_kg FROM dock.parcel` for a role that cannot see `tick_col`. Decode `''` before the label compare. Keep `SELECT 'it''s visible' AS label_ok FROM dock.parcel`.

a168 F1 and F2 are closed on the fresh fixture. NFC labels, calendar phrase labels, and `""` inside quoted identifiers are closed. Details, the `DISTINCT` note, and the `hold_bin` relationship-name collision are in `FINDINGS.md`.

REQ-OOS-05 and `docs/06-ontology-layer.md` section 7.1 are the measure. Cache separation, the independent Rust hash, sample refusal, RLS proof refusal, and the manifest's unsupported labels held. Domains, tokenizer, read API, search, mount, authentication, the policy adapter, and publication gates G1–G13 stay open. This is not SP07-W06 approval. A visibility snapshot is not authentication and not a grant. A successful parse is not SQL execution.

The overlay under `overlay/` was applied only to a copy of the git archive. `Cargo.lock` was not edited. Slot B and PostgreSQL were not used.

`CLEANUP.txt` records removal of this task's network, cargo volume, target volume, and `/tmp` tree. The builder containers were removed by their launch scripts. The pinned Rust image was left in place. `docker prune` was not run. `MANIFEST.sha256` is the SHA-256 of every file in this directory except itself. `source/blobs.sha256` is the fixed-commit content hash and git blob id.
