# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a306`. Reviewed commit and fixed archive are `72dc9e2db841de4de368216bbbe13d738f850ea9`, tree `bf6c8d28dcb47a8dcb1c6cc1e7b6bfc21b963e64`. Helper tree `701743f204495101b5efd20104bccb8d104df467`. Repo HEAD recorded at the smoke launch is `cab5e8e219d1924070df7336f5ab5d4ce7cf40dc`.

This directory is the independent review of `tools/enum-store`. The author session `a293` is not this review. `a278` approved the enum-history helper at archive `7f500ff0a61a42ea606a281dd931099091a52ef6` only. The parent ledger was not edited. This file does not accept SEM-D04 for the parent, does not accept a `docs/05` clock-driven `oo_dict.enum_values` view, does not accept authenticated finance or semantic certification, shared multi-project tenancy, or an owner authenticator, does not close SP08, and does not certify a release.

The approval covers one project bound at install, explicit civil `as_of` snapshots, replay and drift, compare-and-swap activation including one winner of two concurrent activations, SQLSTATE `42501` for a revoked view-owner membership and for runtime access to control, source, and business tables, a `NOLOGIN` view owner, the handwritten historical, retroactive, `95/100`, and `94/100` rows, unknown code `8` as `unexplained` with a null class and a notice, refusal of `inferred_unapproved`, and retention of the historical row across a recorded postmaster restart. `finance_certified`, `semantic_certified`, `owner_authenticated`, `metric_certified`, and `clock_used` stay false.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/` and `source/commit.txt`
- `overlay/a306_enum.rs`
- `tools/lock_subset.rs` and `linux/smoke/lock-subset.stdout`
- `linux/smoke/` for the 8-test author smoke and the pre-overlay Clippy
- `linux/overlay/` for the first postmaster change, the author restart read, `independent_store`, and Clippy on that same archive
- `linux/restart/` for the second postmaster change and `restart_keeps_the_explicit_row`
- `CLEANUP.txt`, `MANIFEST.sha256`, `recipe-origin.txt`

## Decisive exits

`linux/smoke` container `6ad89543b99a5983ba7fe04c29a4f45b6da84b2dc69adb2339678a3215b31901`. `docker-run.exit` 0. `smoke-test.exit` 0, `smoke-count.txt` 8, `clippy.exit` 0, `lock-subset.exit` 0, `lock-summary.exit` 0, `fetch.exit` 0, `product-unchanged.exit` 0, `workspace-probe.exit` 101, `root-manifest-used.txt` `harness`.

`linux/overlay` container `8d0b8ef703a8675b585339b9f22ef476ee528dcd4c20fd7eaea3f9d660874b9c`. `docker-run.exit` 0. Postmaster `2026-10-06 12:02:25.354892+00` then `2026-10-06 12:05:57.404939+00`. `author-restart.exit` 0, `test.exit` 0, `overlay-count.txt` 1, `clippy.exit` 0, `product-unchanged.exit` 0.

`linux/restart` container `25f8e0fd4829c6a7700a2b4993e3ec601ecccf014803bdc0db51a7e607480b80`. `docker-run.exit` 0. Postmaster `2026-10-06 12:05:57.404939+00` then `2026-10-06 12:06:22.319356+00`. `test.exit` 0, `overlay-count.txt` 1, `product-unchanged.exit` 0.

`pg/up.exit` 0. Container `4686624ce3dedf7a93585abc55d5b0f986476250f0e7125c5a9107cd132cd7e3`. Port bindings `{}`.

Each successful builder hostconfig has Memory `6442450944`, NanoCpus `1000000000`, CapDrop `["ALL"]`, Privileged false, OOMKilled false. Smoke, overlay, and restart use NetworkMode `oo-a306-eef831aa-net`.

Child lock `19619c1de9ad8fec8a11b62e4464d93c3789a5ecfdc519281b2f0bc45970c923`. Root lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Registry summary `208/0/0` with three path packages.

Printed independent digests: historical `7b715c151e8b63392c4df73a7c507d09ea5862b834eca53b5174831cd2b3094d`, coverage `95/100` `559091929746e4012afc17176f71c27d5e5f03c8c915beac15ba4b2b07dd6d25`, coverage `94/100` `51400204dd207f3cd866480a3f3a11d8cfc286ae62fc13d694e2539af6043bf1`.

## After the inventory

`linux/manifest/launch.sh` writes `MANIFEST.sha256` from inside the container. It then writes `docker-run.exit`, `hostconfig.txt`, `console.log`, and `container-removed.txt` on the host. `CLEANUP.txt` is written after that container is gone. Those five files are outside the hashed set. The manifest tool also skips its own output path.
