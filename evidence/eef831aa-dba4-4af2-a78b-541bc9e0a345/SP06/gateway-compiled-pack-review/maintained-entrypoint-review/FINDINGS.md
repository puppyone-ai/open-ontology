# Findings

## F1. The maintained entrypoint now completes

The prior review ran `OO_A326_LIVE=1` with `--ignored` and the test panicked at `http_gateway_live.rs:235` with `legacy_unsupported`. Commit `99e93d0f` removes that call. This run, on archive `99e93d0f` and private database `oo_a345`, exited 0. The test passed in 11.36s. `a326-live-marker.txt` is `ran=1` and `completed=1`.

The same command without `--ignored` still reports `0 passed; 0 failed; 1 ignored` and does not create the marker, even with `OO_A326_LIVE=1`. A green default `cargo test` still does not execute this entrypoint.

## F2. The packs are the compiler fixture, and the digest files are removed first

`compile_pack` matches the approved helper. It runs `gateway-pack-fixture compile --compiler role-pack-bundle` on `effective.json` plus `visibility-picker.json` and `visibility-auditor.json`. The test requires the two digest files to be 64 lowercase hex characters, to differ, and to equal each artifact's `pack_digest`. It then deletes both digest files and asserts the pack directory lists only `auditor.json` and `picker.json`. `load_dir` would refuse a leftover `*.digest` name before `listen.addr`. The HTTP assertions ran, so that deletion happened.

`fixture_artifact` is still the loader helper that returns `legacy_unsupported`. This commit does not make it emit a pack.

## F3. The compiled-pack campaign was not repeated

`http_gateway_pg_live.rs` is the same blob as `6001e18e`. The diff of the live test contains no HTTP assertion line and no sleep line. Both `Duration::from_millis(5_200)` calls remain. No concrete defect in the loader, compiler, lock, or `compile_pack` required another run of that test. `pg-live-not-rerun.txt` records that choice. The prior `linux/smoke-3` result remains the cache, revoke, outage, bounds, and tamper evidence. The earlier failed attempts in `linux/smoke` and `linux/smoke-2` were not edited.

## F4. What this run does not show

E45 is still open. This test does not add the 1-second warm-hit check, the tamper process refusals, or the hidden-visibility compiler refusal. Those stay in the previously approved compiled-pack test. `/ready` behavior is whatever the unchanged HTTP assertions already check. This packet does not claim public TLS, OAuth, an IdP, a readiness database probe, durable audit, SQL execution, or runsc.
