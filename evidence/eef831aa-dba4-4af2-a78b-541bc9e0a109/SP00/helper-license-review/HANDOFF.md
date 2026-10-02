# a109 handoff — helper license record and verifier review

**Decision: request-changes. Records stay `audited=false`. Do not set `audited=true` from this review.**

The real bundle check exits 0 and prints `helper-evidence-check: OK records=5 audited=false license_record_status=complete-for-review release_signoff=false`. Three paired forgeries also exit 0: a moved adoption instant, an MIT-to-Apache-2.0 choice, and an audit file that says `"status":"unverified"` once its digest matches the inventory. A 20,000-deep JSON object aborts the checker, exit 134. Those results are in `FINDINGS.md` and `linux/probe/cases.tsv`.

AT-SEC-05, full SP00, the whole graph, image, owners, platform libc, runsc, CI, secrets, customer, and release stay unsigned.

## Bound input

| | |
| --- | --- |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a109` |
| Commit | `abd074af5d59d413e12b8f615c50343e1655047f` |
| Tree | `b67278ca59667699a876cd456ab1c1f7cc191847` |
| Parent | `6cd955af38ce426bb1102a6e078e8580febed776` |
| Subject | `harness: close containment-supervisor license evidence` |
| Helper runtime | `f1a6005971e3dc3075094c87ed670fc9d25e9ae0` |
| Lock sha256 | `14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1` |
| Lock blob | `2152785a3c9dc6713a62eb18052d7224bb5357eb` |
| deny.toml sha256 | `4320807bfe747fe339ce7bf539a1cff5604e8e0c7677760e2cbc7abc1868624b` |
| deny blob | `367598cb3768b8c8af698041cd0f84ce6f8fb7be` |
| Run | `f760a961-386b-46c2-a505-109e54a6748a` |
| Workspace tip at launch | `170dffba8375ca3631df0cc52fdbe65706b702a0` |

The smoke compiled `git archive` of `abd074af` for the checker, the helper manifest and lock, provenance, `ci/deny.toml`, and the a101 raw inputs the checker reads. Snapshot sha256 `bc398585818736a740b15243618aba12370889e05e44b166e4c81ae5eed2c2b1`. The workspace tip was not compiled. `git diff abd074af` for the checker, provenance, helper manifest, helper lock, and `ci/deny.toml` was empty after the run. Helper source against `f1a6005`, excluding `provenance/`, was empty. The same lock blob and deny blob are at `f1a6005`, `abd074af`, and `HEAD`.

## What this run showed

Rust 1.94.0, linux/arm64, image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Container `8e294e65878dc3b77973b2212058bcb44c3a7687281799e96d029c03261ec031`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `airgap_claim=no`: `clippy-component.err` is `info: downloading component clippy`. Test, clippy, and the oracle build then ran `--locked --offline` and exited 0. `worst-including-offline.exit` is 0. No online retry of those three phases was required.

`cargo test --locked --offline`: 3 unit tests and 25 closure tests passed, 0 failed. `cargo clippy --locked --offline -- -D warnings` finished with no warning line. Oracle `hits=38 misses=3 observes=5 cases=46`. Oracle process exit 0. `smoke.exit` 0 means those phases exited 0. It does not mean the three misses passed review.

Phase exits, all integer 0: identity, pin, cargo-test-offline, clippy-component, clippy-offline, oracle-build-offline, oracle-run, smoke, worst-including-offline, docker-exec, driver.

Slot B. Builder count before launch: host cargo/rustc/rustup 0, running Rust-image containers 0. One a109 container. No PostgreSQL. a111 `linux-smoke` phase exits and `docker-run.exit` were integer 0 before this launch.

## Records

Disk `audited=false` on all five files, before and after. Hashes:

- bitflags `d439576a82d931be681532afab928d3f08d884845982efe9ca8f80f03a20977f`
- cfg-if `183de8fc202fd80fc19ac3b7dbf82563406d91fed41b090be8eef97612255dc4`
- cfg_aliases `673847c4d429c2347db5c667bcbb0665549fe2b56fb15918f131712f8c57aabc`
- libc `3c27783c2e435a7c6c86a2a037dd8aba86161e7f54a24d1bcb9ad80210548218`
- nix `242bb25b83a77edb8535e29d027fe94d18119a076525ce8808ddf11bf5a24367`

`audited_record_errors` on disk reports only `audited record flags`. An in-memory flip to `audited=true` returns zero errors from that function. That is schema shape. It is not policy admission, an owner appointment, or a fresh scan.

Adoption on disk is `2026-10-02T11:10:37Z`. Choices on disk are MIT for the three OR crates. cfg_aliases MSRV is the explicit unknown string. libc Apache has no appendix. The historical scan is the a101 file recorded `2026-10-02T12:12:14Z`, RustSec `117edb3bed98e9be112f277b7615eea3252e7c43`, cargo-audit 0.22.2, cargo-deny 0.20.2. `fresh_current_scan` is false.

## Author attempts left as they were

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a105/SP00/helper-license-closure/raw/` was not edited. attempt-1 `docker-exec.exit` is 101. `attempt-2-status.txt` says `FAILED verifier-test`. attempt-5 `verifier-clippy-retry.exit` is 101. The author's later `verifier-test.exit` and `verifier-clippy.exit` are 0. Those 101s stay 101.

## Cleanup

Removed only this run: container `eef831aa-dba4-4af2-a78b-541bc9e0a109-rust`, network `oo-a109-f760a961-386b-46c2-a505-109e54a6748a-net`, volumes `oo-a109-f760a961-386b-46c2-a505-109e54a6748a-cargo` and `-target`, and `/tmp/oo-a109-f760a961-386b-46c2-a505-109e54a6748a`. Extracted crate bytes in `/tmp/oo-a109-upstream` were deleted after `upstream/crate-compare.txt` was saved. No prune. No product commit.

`linux/docker-ps-after.txt`, written by that cleanup, still lists `eef831aa-dba4-4af2-a78b-541bc9e0a107-pg` as Up. A later `docker ps -a` in this session does not list the a107 containers. The a109 stop and rm logs name only the a109 container.

## Coordinator

Leave `audited=false` until a later change anchors adoption to the helper-lock commit time, pins the OR choice, and rejects an unverified audit status even when the digest matches. The depth abort needs a clean bound before this checker is treated as robust against hostile records. This handoff does not sign the parent task.
