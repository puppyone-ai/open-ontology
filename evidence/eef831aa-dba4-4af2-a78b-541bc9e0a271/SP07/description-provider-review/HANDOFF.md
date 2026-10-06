# Handoff

Verdict for provider commit `aba067efe4b19bae868e15e23157ebbcf0a4969f` in fixed archive `9951cda7f128e30f1f6ce02e409d542d20a9e4fb`, tree `7e83a381c8a90c5f61f3ce73947eb7ac441381e9`:

- S5-D03 HTTPS description provider: **APPROVE_LIMITED**

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a271`. No product file was edited. No dependency file was edited. The parent ledger was not edited. This verdict does not accept full S5, the parent goal, or a release. a264 APPROVE_LIMITED for S5-D01 and S5-D02 stands. a263 is the author record and is not this approval.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `ORIGIN.md` for the a204, a200, a199, a253, a263, and a264 recipe hashes
- `source/blobs/` for the fixed provider, the repaired library, and both locks
- `overlay/provider_a271.rs`, content SHA-256 `6cec0432423460bc3d41d551418cfdb3c05be03af322e530dfadd67c66f18414`
- `linux/smoke/` for the preserved `cargo fetch -p` failure
- `linux/smoke-2/` for the 25 tests before the overlay
- `linux/proof/` for the decisive TLS run and Clippy
- `CLEANUP.txt` and `SHA256SUMS`
- `tools/sha256_manifest.rs`

## Decisive exits

`linux/smoke-2`, before the overlay: `docker-run.exit` 0, `smoke.exit` 0, offline fetch 101, locked fetch 0, library 2 passed, author integration 23 passed in 6.11s, lock subset `registry=248 path=4 missing=0 mismatch=0`, tamper `mismatch=1`, locks unchanged. Container `f90aebde9885c47d50d7203e3d713a92088a91ea9265f4f1285e360045e12c24`. OOMKilled false. Privileged false. Memory 6442450944. NanoCpus 1000000000. Clippy was not run.

`linux/proof`: `docker-run.exit` 0, `proof.exit` 0, `test-lib.exit` 0 (2 passed), `test-provider.exit` 0 (23 passed in 6.07s), `test-a271.exit` 0 (7 passed in 2.48s), `clippy-component.exit` 0, `clippy.exit` 0, `lock-hash.exit` 0, `tree.exit` 0, `source.exit` 0. Container `dfb25ef45420d29a0996d7479acfec178f67627fa4f040cff69097ae87981dbd`. OOMKilled false. Privileged false. Memory 6442450944. NanoCpus 1000000000. Root lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Provider lock `d6b28e14bceb66512cc44acbbdce9af84754c860bef199384eb0b1662f9258cb`.

## Preserved attempt

`linux/smoke`: `docker-run.exit` 1, `smoke.exit` 1, `fetch.exit` 1. `fetch.log` says `error: unexpected argument '-p' found`. Container `7e301b88036ddd00eec43acca5da98071fe0eff4818e14c62c63861e02d68c63`.

## Do not widen this review

Compiler statements stay `not_used`, `not_approved`, and `not_metered` after a performed call. The call is on `transport`. Certificates stay false. A compiler budget failure stays `budget_reason=caller`. The shop order on this fixture matches both the prepared request and alphabetical order, so it is not a new sort proof.

Column-level candidates, a real provider, and a residency certificate need their own frozen contract before code. This handoff does not open a child.

Own containers, volumes, the private network, and `/tmp/oo-a271-eef831aa-dba4-4af2-a78b-541bc9e0a271` are gone. `oo-a274-chain` and `oo-a274-pg` were seen at the proof launch and were not touched.

`SHA256SUMS` is the inventory from `linux/manifest/finalize.sh`. The compile log under `linux/manifest/` is the earlier run, from before this handoff existed.
