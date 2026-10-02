# Report — a109 independent review of helper license records and the std checker

Scoped verdict: request-changes. The five registry records remain `audited=false`. Smoke exit 0 is the Rust 1.94 build, the author's tests, clippy, and an oracle process that exited 0 after recording three misses. The misses are the review result. Details are in `FINDINGS.md`. The handoff is `HANDOFF.md`. Hashes are in `HASHES.sha256`.

## Scope

Session `eef831aa-dba4-4af2-a78b-541bc9e0a109`. Binding commit `abd074af5d59d413e12b8f615c50343e1655047f`, tree `b67278ca59667699a876cd456ab1c1f7cc191847`, parent `6cd955af38ce426bb1102a6e078e8580febed776`, subject `harness: close containment-supervisor license evidence`, commit time `2026-10-02 21:52:18 +0800`.

Reviewed the std-only crate `testing/harness/helper-evidence-check` and the five records under `testing/harness/containment-supervisor/provenance/audit-records/`. Helper runtime commit `f1a6005971e3dc3075094c87ed670fc9d25e9ae0` is unchanged outside `provenance/`. Lock sha256 `14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1`. `ci/deny.toml` sha256 `4320807bfe747fe339ce7bf539a1cff5604e8e0c7677760e2cbc7abc1868624b`.

Writes stayed in `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a109/SP00/helper-license-review/` and the container's private volumes. No product, manifest, lock, policy, spec, plan, stage, or old-evidence edit. No commit. No subagent. No worktree. No push.

a111 `linux-smoke` exits (`cargo`, `clippy-component`, `clippy`, `fetch`, `rustc`, `test`, `worst`) and `docker-run.exit` were the integer 0 before slot B was used. Slot A stayed with a107. This task ran one Rust builder. No PostgreSQL client or server was started.

## Provenance that was rechecked

Workspace tip at launch was `170dffba8375ca3631df0cc52fdbe65706b702a0`. It was not compiled. The container archive was `git archive` of `abd074af` for the checker, the helper `Cargo.toml` and `Cargo.lock`, `provenance/`, `ci/deny.toml`, and the a101 raw inputs named by the checker flags. Snapshot sha256 `bc398585818736a740b15243618aba12370889e05e44b166e4c81ae5eed2c2b1`.

After the run, `git diff abd074af` for those product paths was empty. Lock blob `2152785a3c9dc6713a62eb18052d7224bb5357eb` and deny blob `367598cb3768b8c8af698041cd0f84ce6f8fb7be` match at `f1a6005`, `abd074af`, and `HEAD`.

Retained scan input hashes, recomputed, match the pins:

- `scan-audit.out` `f64e43716f72c173879cb30130f73df0f1c802ac32960eb6c3955c15c2209f81`
- `scan-deny-advisories.err` `02d186150402cda84d64b50da1dea4b586274b3afbd3fbaf078a7c4e389920c3`
- `age.txt` `8ace2a347494e1767d9a9ac7d771f6e806aacba052fa2b55da920bef276d5160`
- `helper-metadata.out` `6d1a3456f8781bc18989553ea9362109c39c97aff480636a7251bc28cab60186`
- `advisory-rev.txt` `ee376fcdd344d86d428ff4f80997e660fe2b2217fdf77139ddfcb5b6e3055cbd`

Tools named by the records: cargo-audit 0.22.2, cargo-deny 0.20.2, RustSec `117edb3bed98e9be112f277b7615eea3252e7c43`, commit time `2026-10-02T10:58:33+02:00`, recorded `2026-10-02T12:12:14Z`. This review did not rerun the audit or deny matrix. `fresh_current_scan` remains false. The scan is the retained a101 output.

`f1a6005` author and committer times are both `2026-10-02T19:10:37+08:00`, which is `2026-10-02T11:10:37Z`. That is the adoption instant stored on the five records. Completed ages at that instant are bitflags 22, cfg-if 15, libc 72, cfg_aliases 77, nix 144. The oracle checked those five vectors against its copy of the civil calendar and then ran the cases.

## Upstream

`upstream/crate-compare.txt` records downloads from `https://static.crates.io`. All five crate checksums match the lock. Every `LICENSE`, `LICENSE-MIT`, `LICENSE-APACHE`, and cfg_aliases `NOTICES.md` was byte-equal to the canonical provenance file and to the a101 retained copy.

libc's Apache text ends at `END OF TERMS AND CONDITIONS` and has no placeholder appendix. bitflags and cfg-if share the Apache file that still contains `Copyright [yyyy] [name of copyright owner]`. The records call that `standard-placeholder-not-a-named-owner`. cfg_aliases ships no `rust-version`. Its canonical `NOTICE` equals upstream `NOTICES.md` (`1e2b7ade3fb228130408b9990cae6a7618eb314c75aa0b164bfe485d9d9756ee`).

## Governance call

`audited_record_errors` and `template_errors` were imported from `governance/qa_record.py` and run in memory. Disk files were not written. Each record has `disk_audited=False` and the single disk error `audited record flags`. Memory with `audited=true` has `memory_flip_audited_true_error_count=0`. `template_errors` is 26 on four records and 25 on cfg_aliases, which is the blank-template checker looking at filled records.

That zero is schema eligibility for the field shape the function knows. It does not read `open_gates`, and it does not open license bytes. All ten gates are `open`. `admission` is false. `release_signoff` is false. `reviewers_assigned` is empty.

`governance/check_governance.py --self-test` does not read these records. It was not used as the shape check. The author's a105 self-test log was left as it was.

## Linux arm64 Rust 1.94

Run `f760a961-386b-46c2-a505-109e54a6748a`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, pull never, platform linux/arm64. Container `8e294e65878dc3b77973b2212058bcb44c3a7687281799e96d029c03261ec031`.

`uname` `Linux 8e294e65878d 6.12.76-linuxkit #1 SMP Fri Apr 17 14:56:37 UTC 2026 aarch64`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, commit `4a4ef493e3a1488c6e321570238084b38948f6db`, host `aarch64-unknown-linux-gnu`. `RUSTUP_TOOLCHAIN=1.94.0`. The product `rust-toolchain.toml` was not what executed.

Host config: privileged false, cap-drop `ALL`, `no-new-privileges`, memory and swap 8589934592, nano CPUs 4000000000, pids 4096, network `oo-a109-f760a961-386b-46c2-a505-109e54a6748a-net`, archive mounted read-only.

`airgap_claim=no`. Clippy component stderr is `info: downloading component clippy`, exit 0. The following were `--locked --offline` and exit 0, with no online retry:

- `cargo test`: unit tests 3 passed (`timeutil` two, `check::placeholders_are_not_duties` one), main tests 0, closure tests 25 passed, doc tests 0. Compile of the test profile 0.92 s.
- `cargo clippy -D warnings`: `Finished dev profile` in 0.18 s, stdout empty.
- oracle `cargo build --locked --offline`: `Finished dev profile` in 0.37 s.

Checker binary `c2a7b9bae81198010c0cfa6b682a71087a2df3541c14c16fd7df7f17d4ca8694`. Oracle binary `24dd7af84ce0361d43017ebd392f5ba3d414cdb0c953af5cd58009056e8ebad7`.

Integer phase exits: identity 0, pin 0, cargo-test-offline 0, clippy-component 0, clippy-offline 0, oracle-build-offline 0, oracle-run 0, smoke 0, worst-including-offline 0, docker-exec 0, driver 0.

Builder count recorded before `docker run`: `host_rust_comm=0`, `rust_image_containers=0`, `total=0`.

## Oracle

`oracle/` is a separate package, edition 2021, `publish=false`, own `[workspace]`, no dependencies, `forbid(unsafe_code)`. It spawns the real checker with the required flags, pipes stdout and stderr, and kills the child on the case timeout. Judgement rows are `linux/probe/cases.tsv`. Summary: `hits=38 misses=3 observes=5 cases=46`.

The three misses, all checker exit 0 and the OK line:

1. Adoption rewritten to `2026-09-24T17:36:11.278420Z` with recomputed ages. The lock commit time is not an input.
2. Inventory choice, bitflags `dual_license_choice`, and the first nix native choice set to Apache-2.0 together. MIT is not pinned outside that pair.
3. Audit JSON gained `"status":"unverified"` and the inventory digest was updated to match. `check_audit_file` does not read `status`.

Depth 64 exited 0. Depth 20,000 aborted, exit 134, stack overflow, 25 ms. `json.rs` limits array length and object key count and does not limit depth.

The positive case on the untouched archive exited 0 in 46 ms with `audited=false` and `release_signoff=false`. Lock checksum, version, repository, `version.created_at`, features, RustSec identity, obligations, false clearance flags, missing and hash-aligned damaged Apache and cfg_aliases notice, traversal, duplicate keys, UTF-8, NaN, null, symlink, fifo, oversize, and file count failed closed. Exit codes and timings are the table in `FINDINGS.md`.

`sha.rs` hashes with a fixed `/usr/bin/sha256sum` (this image has it; sha256 `7911a1f0aec4678c540bd78ff886f48d42790c8689a82ed0ae25ed79ed5dd512`) or else `/usr/bin/shasum -a 256`. `symlink_metadata` must be a regular file. A `sha256sum` planted earlier on `PATH` did not run: the marker file was absent. The hash child has no timeout and inherits the process environment. This run's fifo and symlink cases returned in 23–27 ms.

`read_bounded` caps at 1_048_576 and uses `symlink_metadata`. `Path::is_file` is what rejects a fifo before that read, so the license fifo is exit 2 and the lock fifo is exit 10.

Unchecked or weakly checked fields, as executed: adoption versus git, OR choice versus a fixed MIT, audit `status`, unknown JSON keys, hardlink `nlink`, and the helper manifest beyond the single line `license = "Apache-2.0"`. Validated against the lock, the age file's `version_created_at`, the version JSON, cargo metadata, and the raw audit and deny bytes: version, checksum, repository, publication, features, RustSec commit, vulnerability emptiness, and deny error/warning counts. Mirrored hashes in the inventory are caller-supplied. The positive path checks them against the files. F3 shows a matching pair of forged digest and forged bytes still passes when the unread `status` field is the only semantic change.

## Author history

The a105 raw directory was not modified. attempt-1 `docker-exec.exit` remains 101. `attempt-2-status.txt` remains `FAILED verifier-test`. attempt-5 `verifier-clippy-retry.exit` remains 101. The author's final `verifier-test.exit` and `verifier-clippy.exit` remain 0. This review does not call the 101s green.

## Cleanup

The launch trap stopped and removed `eef831aa-dba4-4af2-a78b-541bc9e0a109-rust`, the run's network, and the two run volumes, then deleted `/tmp/oo-a109-f760a961-386b-46c2-a505-109e54a6748a`. `/tmp/oo-a109-upstream` was removed after the comparison log was on disk. No `docker prune`. `docker-ps-owned-after.txt` is empty.

`docker-ps-after.txt` from that trap lists `eef831aa-dba4-4af2-a78b-541bc9e0a107-pg` Up. A later `docker ps -a` does not list a107 containers. The a109 rm log names only the a109 container.

## Still unsigned

AT-SEC-05, full SP00, whole graph, image, owners, platform libc, runsc, CI, secrets, customer, and release. The checker is not a scanner, a legal owner, a distribution bundle, or a corporate approval. Records stay `audited=false` for the coordinator.
