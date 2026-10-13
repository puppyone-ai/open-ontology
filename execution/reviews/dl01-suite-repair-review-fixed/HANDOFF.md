# Handoff

## Verdict

DL01-W1-07 is `request_changes` on source `0d0dc60bdb1c1d40eab7888428f16dbd53b0e805` and evidence seal `1b5e2c4e950aaab7b0615fcf3258a197ce09f602`. The prior review of this session, `c6dc59912c2b2f5dee55832c73e6b43d71801fa2`, stays the git record of the last-wins duplicate grant. The caller-facing command remains:

```text
source-conformance check --matrix FILE [--historical-pg DIR] [--nightly FILE] [--package DIR]...
source-conformance map
```

Exit 0 means the report has no error findings. Exit 1 means a claim or evidence finding. Exit 2 is usage. `promoted` and `matrix_rewritten` stay false on the reports this review collected.

The unmet exit is nightly suite credit for a package that fails to load. A parseable package that omits `service_version` exits 1, fails cell `rev-w37-missing-valid`, and reports `version.identity_missing`, `claim.manual_assertion`, and `claim.missing_test`. Three suite days citing evidence id `rev-w37-missing-valid-0001` still produce trailing consecutive success days 3 and `suite_runs` 3. A numeric `service_version` `16.9` does the same with `version.identity_malformed` on cell `rev-w37-malformed`. An invalid JSON manifest does the same with `evidence.malformed` on cell `rev-w37-missing`.

The duplicate-key grant from `c6dc5991` is closed on this binary. `service_version` `15.2` then `16.9`, `service_version` `16.9` then `15.2`, `exact_version` `15.2` then `16.9`, and `exact_version` `16.9` then `15.2` each exit 1 with `version.identity_duplicate` and `claim.missing_test`. Equal duplicates, `\u0073ervice_version`, `service_\u0076ersion`, a repeated key inside one identity object, and two identity objects deny the same way. A loaded duplicate nightly file records trailing 0 and `suite_runs` 0. The distinct-field case `exact_version` `16.9` and `service_version` `15.2` exits 1 with `version.identity_mismatch` and `claim.missing_test`, and its nightly credit is 0.

The review binary was Linux arm64 debug `source-conformance`, sha256 `8af4607a7c65ad5a1867b0040d5620a042fd8b1a58bd5f3d55117337a2400bd1`, size 4954336. It was built from the dependency archive plus `tools/source-conformance` at the source SHA, then removed with the private target volume. Raw probe files and argv are in `linux/probes/`.

## What a later repair must change

Withhold nightly suite credit when the cited package fails to load. Cover a valid missing `service_version`, a non-string `service_version`, and a manifest that is not JSON. Keep each of those cells failed and keep the runtime claim withheld. Keep the loaded-duplicate nightly credit at trailing 0 and `suite_runs` 0. Keep the distinct-field deny for `16.9` claimed against service `15.2`. Leave experimental, designed, manual, nightly, cloud, hash, and missing-test behavior as this packet measured it on the probes that already match. Leave `promoted` and `matrix_rewritten` false. A narrower `CONTRACT.md` does not replace that exit. This review does not ship a patch.

Leave `history/c6dc5991/missing/package/manifest.json` invalid. Its hash is `5147f25da320baf282701be958a1597b77469e6f41bbcb36a6002ffaaa3be126`. The regenerated invalid `harness/fixtures/missing` file is the repeat of that history. The valid missing-field probe is `harness/fixtures/missing-valid`.

## What this run already shows

The checker refuses a bad matrix schema, a bad package schema, a raw hash mismatch, a tampered CC-01 digest, and a package whose target is absent from the matrix. It accepts one experimental cell when the mapped CC cases, raw hashes, raw exit 0, exact version, service identity, and cloud scope agree. Designed and candidate cells do not require suite packages. It does not promote a designed cell from a 14-day streak, does not treat a manual run as a suite day, and recommends `experimental` after two trailing suite failures on a verified cell while leaving the matrix file unchanged. Managed variant `rds` cannot use container evidence for a runtime claim. The sealed historical PostgreSQL draft does not satisfy the CC suite. Family and product come from the matrix target. Postgres does not accept `8.0.32-log` as `16.9`. Mysql does accept `8.0.32-log` as `8.0.32`. Shorter tokens, foreign suffixes, `mysql-compat`, and `19.0.0.0.0` versus `19c` do not match.

Compile the same way: `git archive 577160c22de63adb9b38e0147f3bc2da893c4d9d`, overlay only `tools/source-conformance` from `0d0dc60bdb1c1d40eab7888428f16dbd53b0e805`, and do not mount the live tree. Core, builder, `contracts/schema/oos-v1.json`, and `tools/source-semantics` stay on the base archive. The source blob of `oos-v1.json` differs from the base blob. This compile kept the base blob `48432a164cba1e42c28790ad499850a8f3b76abc0b3fb863d9538b6dbe279462`. The source commit contains `tools/source-support` and `tools/source-semantics`. This child does not compile those trees. Place the archive on a Docker Desktop shared path. `/tmp` is not shared, and a bind of `/tmp/oo-dl01w1-37/src` returned 125 with no container.

## What stays outside this verdict

DBX-W14 remains open. Matrix integration remains pending. The current matrix still has zero verified and zero certified cells, and this review does not change that. No source capability grade, verified grade, 14-night promotion, or release is accepted. The frozen child itself is not approved. `parse_support_matrix` remains absent. Source-support `29597079f7feacc6e3934b5c803686fc80567a93` is accepted and is an ancestor of this source, and public API consumer integration stays outside this repair. This review adds no designed stub. The sealed author attempt exits 101 and 127 stay in the author packet as history.

## Resources

Removed at `2026-10-09T18:41:10Z`, each exit 0: container `oo-dl01w1-37-build`, network `oo-dl01w1-37-net`, volumes `oo-dl01w1-37-cargo` and `oo-dl01w1-37-target`, `/tmp/oo-dl01w1-37`, and `/Users/redux/.cache/oo-dl01w1-37`. No database was created. The retained image is `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Foreign builders were not stopped. One builder ran after the refused mount. It is gone. `linux/docker-run.exit` still reads 125 because the success path does not rewrite that file. The successful start is container `1781350b333a8adc7b24d045893fdacc5d2457551ec631d8370952524b53e4e3`, empty `docker-start.stderr`, admit 0, docker wait 0, and inspect `ExitCode=0`.

## Next step

A repair of the unloadable-package nightly credit can be reviewed again against this packet's `missing-valid-nightly`, `malformed-nightly`, and `missing-nightly` probes. Acceptance of the child waits on that withhold, together with the duplicate-key deny this binary already shows. Closing DBX-W14, promoting a matrix grade, integrating `parse_support_matrix`, or treating these fixtures as a 14-day source proof is separate work.
