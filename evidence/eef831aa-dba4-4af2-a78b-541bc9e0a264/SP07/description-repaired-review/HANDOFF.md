# Handoff

Verdicts for fixed `567d621e2e52b7e0011bdb025dc9337e92ecc5a6`, tree `b3a0fd3155ebb985be8faeb75f51978acdc75aae`:

- S5-D01 public library: **APPROVE_LIMITED**
- S5-D02 compiled `description-candidate` CLI: **APPROVE_LIMITED**

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a264`. No product file was edited. The parent ledger was not edited. These verdicts do not accept full S5, the provider, the parent goal, or a release. a257 REQUEST_CHANGES on `fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a` stands.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `ORIGIN.md` for the a204, a200, a199, and a253 recipe hashes
- `source/blobs/` for the fixed producer, CLI, contracts, lock, and tests
- `overlay/description_candidate_a264.rs`, content SHA-256 `f692589f6dbbbfd97b24f5475695981f317ee7c386dc675e8aa33ab18444f12e`
- `linux/smoke/` for the MSRV run before that overlay
- `linux/proof/` for the preserved oracle failure, including `description_candidate_a264.rs` at `b3fc1b89d4cf0d950131a0689eeff2038db351a9bc4bfc141aa109152a525c01`
- `linux/proof-2/` for the decisive run
- `CLEANUP.txt` and `SHA256SUMS`
- `tools/sha256_manifest.rs`

## Decisive exits

`linux/proof-2`: `docker-run.exit` 0, `proof.exit` 0, `test-a264.exit` 0 (`physical_a_and_b_library_and_cli` ok, 0.19s), `test-lib.exit` 0 (2 passed), `test-description.exit` 0 (10 passed), `test-order.exit` 0 (1 passed), `test-cli.exit` 0 (5 passed), `clippy-component.exit` 0, `clippy.exit` 0, `lock-hash.exit` 0, `tree.exit` 0, `source.exit` 0. Container `cf05571ca983496aa7726c45e57ead1be9c222d238ed4e67690586476af69b8f`. OOMKilled false. Privileged false. Memory 6442450944. NanoCpus 1000000000. Lock `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

`linux/smoke`, before the overlay: `docker-run.exit` 0, `smoke.exit` 0, offline fetch 101, locked fetch 0, the same focused tests passed, Clippy not run. Container `e2ef6bb117536c9d36a854f57ddbf11a05b1309b7d84fe2f77af09d89733af3f`.

## Preserved attempt

`linux/proof`: `docker-run.exit` 101, `proof.exit` 101, `test-a264.exit` 101 at line 547. The fixture compared the response-byte `request_sha256` with the full-budget request hash. Focused tests and Clippy in that container exited 0. Container `5764a1b000aeec08c6d88e2dff378f861bd28f88d42ebaf62e38b9ca9e91c4bc`.

## Do not widen this review

Column-level candidates, provider transport, residency, cost, and a model call need their own frozen contract before code. a263 remains that provider task. This handoff does not open a child and does not review a263.

`request_sha256` stays null on the early table budget and the request-byte budget. Response-byte `request_sha256` is the hash of the request issued with that budget.

Own containers, volumes, the private network, and `/tmp/oo-a264-eef831aa-dba4-4af2-a78b-541bc9e0a264` are gone. `oo-a263-proof` was seen at the proof-2 launch and was not touched.
