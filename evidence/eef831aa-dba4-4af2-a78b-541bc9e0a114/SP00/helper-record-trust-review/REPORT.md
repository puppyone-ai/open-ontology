# Report — a114 independent review of helper record trust

Verdict for F1–F4: **APPROVE LIMITED**. The five records stay `audited=false`. `admission`, `fresh_current_scan`, and `release_signoff` stay false. This review does not certify hostile-input robustness, a full license reading, a legal opinion, a fresh scan, or a release.

The fixed archive's canonical check exits 0 and prints `helper-evidence-check: OK records=5 audited=false license_record_status=complete-for-review release_signoff=false`. The paired forgeries and the depth bounds below failed closed with typed exits and no signal. Details are in `FINDINGS.md`. The handoff is `HANDOFF.md`.

## Scope

Session `eef831aa-dba4-4af2-a78b-541bc9e0a114`. Reviewed commit `6d2f7cdb935c7debad0dbb5292074f325d1948cb`, tree `dad005c861fca7fd906f49551169777a37b1c5d0`, parent `d67795e397657087495a32317938ecdd7a6cb028`, subject `harness: pin helper record trust and json depth`. `git rev-parse` of that commit, its tree, and its parent matched those three values before the container started.

The container compiled `git archive` of that commit for `testing/harness/helper-evidence-check`, the helper `Cargo.toml` and `Cargo.lock`, `provenance/`, `ci/deny.toml`, and the a101 raw inputs the checker reads. The archive has no `crates/oo-connectors` tree. Workspace tip at launch was `e5a066622e0409762fe44f11c4d303d2468a3290`. `git diff 6d2f7cdb HEAD` for the checker, provenance, helper lock, and `ci/deny.toml` was empty. That tip was not compiled. The connector workspace was not compiled, staged, or modified.

Writes stayed in this directory and in the run's private volumes and `/tmp/oo-a114-9910ff22-66f3-49c6-82e4-bea42642b58f`. No product, manifest, lock, vendor, ledger, spec, or plan edit. No push.

## Identity that was rechecked

Helper-lock commit `f1a6005971e3dc3075094c87ed670fc9d25e9ae0`, author time and committer time `2026-10-02T19:10:37+08:00`, which is `2026-10-02T11:10:37Z`. Lock blob `2152785a3c9dc6713a62eb18052d7224bb5357eb`. Archive lock sha256 `14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1`.

Parent blobs of `check.rs`, `json.rs`, `lib.rs`, `main.rs`, `fail.rs`, `sha.rs`, `timeutil.rs`, `tests/closure.rs`, `Cargo.toml`, and `Cargo.lock` match `abd074af5d59d413e12b8f615c50343e1655047f`. Content sha256 of those unfixed sources matches the a109 `HASHES.sha256` lines. `trust.rs` and `tests/trust.rs` are absent at the parent. Fixed blobs: `check.rs` `0f97fefa2c16d5add02225d3882ebd7f7bc82106`, `json.rs` `0b4be35dd0975917bc419c78d8fea517f5f0c617`, `lib.rs` `e63d67b9f43c7381239589615e287b2929ecf21c`, `trust.rs` `015356112803046203033568cbedd305f2584399`.

Canonical `scan-audit.out` sha256 `f64e43716f72c173879cb30130f73df0f1c802ac32960eb6c3955c15c2209f81`. Parsed top-level keys have no `status`. `database.last-commit` is `117edb3bed98e9be112f277b7615eea3252e7c43`, `last-updated` is `2026-10-02T10:58:33+02:00`, advisory-count 1280, dependency-count 6, vulnerabilities `found=false`, `count=0`, `list=[]`. Those bytes were not rewritten. cargo-audit and cargo-deny were not rerun.

`version.created_at` from each `version-*.json` object, taken before `published_by`, matches the inventory publication date and the age file. The probe's civil calendar and a host datetime calculation agree:

| package | version.created_at | completed days | forged days | hours |
| --- | --- | --- | --- | --- |
| bitflags | 2026-09-10T01:11:38.500809Z | 22 | 14 | 537.983 |
| cfg-if | 2026-09-16T17:36:11.278420Z | 15 | 8 | 377.574 |
| cfg_aliases | 2026-07-16T14:57:16.699789Z | 77 | 70 | 1868.222 |
| libc | 2026-07-21T21:33:28.018110Z | 72 | 64 | 1741.619 |
| nix | 2026-05-11T07:01:18.650221Z | 144 | 136 | 3460.155 |

Stored ages equal the completed days. Every elapsed interval is above 168 hours. The age file's `floor_seconds` is 604800. The forged instant `2026-09-24T17:36:11.278420Z` still leaves every crate above 168 hours, so an age-floor check alone would accept that pair. The fixed checker rejects the instant.

## a109 RED binding

The retained oracle still hashes to the a109 record: `oracle/src/main.rs` `4a9b07be370f4a251762ed7af609b9fb330e2da276dc9cbe49c45ec2a4f9530a`, `linux/probe/cases.tsv` `c53da068d65c9fdccc7b6f64354035701d92a019f28fb4c7ff1d2bc2a336daf2`, summary `hits=38 misses=3 observes=5 cases=46`. Unfixed checker content matches that review. The 46-case matrix was not rerun. The five retained rows are quoted in `FINDINGS.md`.

## Linux arm64 Rust 1.94

Run `9910ff22-66f3-49c6-82e4-bea42642b58f`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, pull never, platform linux/arm64. Container `6c5b279f1b4cc71a233f26e6c0d60198b28dfa6a3053dbf698e5cdb12d420e36`, start `2026-10-02T21:38:17Z`.

`uname` `Linux 6c5b279f1b4c 6.12.76-linuxkit #1 SMP Fri Apr 17 14:56:37 UTC 2026 aarch64`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, commit `4a4ef493e3a1488c6e321570238084b38948f6db`, host `aarch64-unknown-linux-gnu`. `cargo 1.94.0 (85eff7c80 2026-01-15)`. `RUSTUP_TOOLCHAIN=1.94.0`. rustc and cargo binary sha256 `9732d6c5e2a098d3521fca8145d826ae0aaa067ef2385ead08e6feac88fa5792`. `/usr/bin/sha256sum` sha256 `7911a1f0aec4678c540bd78ff886f48d42790c8689a82ed0ae25ed79ed5dd512`.

Host config, without environment: privileged false, cap-drop `ALL`, `no-new-privileges`, memory and swap 8589934592, nano CPUs 4000000000, pids 4096, network `oo-a114-9910ff22-66f3-49c6-82e4-bea42642b58f-net`. Archive mounted read-only. Private cargo and target volumes. `tmpfs` for `/tmp` and `/work`. No host network, docker socket, host HOME, or shared cargo cache.

`airgap_claim=no`. `clippy-component.err` is `info: downloading component clippy`, exit 0. Test, lib/bin clippy, test clippy, and the probe build then ran `--locked --offline` and exited 0. No online retry of those phases was started.

`cargo test --locked --offline`: lib unit tests 7 passed (`placeholders_are_not_duties`, three json tests, two timeutil tests, `mit_or_null_is_the_baseline_choice`), bin tests 0, closure tests 25 passed, trust tests 11 passed, doc tests 0. Zero failed.

`cargo clippy --locked --offline -- -D warnings` finished with no warning. `cargo clippy --locked --offline --test trust --test closure -- -D warnings` finished with no warning.

`cargo clippy --locked --offline --all-targets -- -D warnings` exited 101. The only error is `src/timeutil.rs:112` `clippy::assertions_on_constants` on `assert!(SEVEN_NS - 1 < SEVEN_NS)`. That file's blob is the parent blob. It was not edited. This exit is outside `smoke.exit`. All-target clippy is red.

Checker binary sha256 `2cb04ba6701778408a5c09839af053c89d50614510546dff7a9e4ae1ec6adad7`. Probe binary sha256 `7ab97c9e855c408fcbb95fa34e10f971c61d76f7cbec1a6a037bcaa48a10be78`. Probe source sha256 `2293ae42758092a2590b0568692d3489780124a2250876fc087eefd8b0c84d78`.

Integer phase exits: identity 0, pin 0, cargo-test-offline 0, clippy-component 0, clippy-offline 0, clippy-tests-offline 0, probe-build 0, probe 0, smoke 0, docker-exec 0, driver 0. clippy-all-targets 101.

## Independent probe

`expected-outcomes.tsv` and `age-floor-preregistration.tsv` were written before `docker run`. After the run, `expected-unchanged.txt` says `expected-outcomes unchanged`. The probe is a separate std-only package. It spawns the archive binary. It does not call the author's test functions. Summary: `matched=22 case_mismatches=0 other=0 missing=0 cases=22`. Each row in `linux/probe/cases.tsv` has `verdict=match`, a typed exit, and `signal=none`. Per-case stdout, stderr, and meta files are under `linux/probe/cases/`. The runner exit is 0 and the 22 rows agree with it. No row is missing.

Container depth counts `{` and `[` only. The probe measured that depth on the document it spawned. Depth 64 object, array, and mixed exited 0 on the OK line. Depth 65 and 20000 for the same three shapes exited 22, `code=bound`, `detail=json depth exceeds 64`, in 20–23 ms, signal none. A scalar root measured depth 0 and exited 21, `detail=missing package`, with no depth-bound text and no signal. Malformed escape and invalid UTF-8 exited 21. A duplicate key exited 14.

## Capacity and cleanup

Before `docker run`, host `cargo`/`rustc`/`rustup`/`rustdoc` count was 0. `docker ps` showed one running container, `eef831aa-dba4-4af2-a78b-541bc9e0a112-pg`. Its name and image do not match the rust count, so `rust_image_containers=0` and `total=0`. The cap refuses a start at total >= 2. This review started one Rust container. Slot B had no a113 container. Slot A names were recorded and left in place.

Cleanup removed container `eef831aa-dba4-4af2-a78b-541bc9e0a114-rust`, network `oo-a114-9910ff22-66f3-49c6-82e4-bea42642b58f-net`, volumes `oo-a114-9910ff22-66f3-49c6-82e4-bea42642b58f-cargo` and `-target`, and `/tmp/oo-a114-9910ff22-66f3-49c6-82e4-bea42642b58f`. `docker-ps-owned-after.txt` is empty. No prune. `docker-ps-after.txt` still lists `eef831aa-dba4-4af2-a78b-541bc9e0a112-pg` Up, `eef831aa-dba4-4af2-a78b-541bc9e0a112-rust-r1` Exited (0), `eef831aa-dba4-4af2-a78b-541bc9e0a112-rust-base` Exited (101), and the exited a064 containers.

## Still unsigned

Unknown record keys, hard links, manifest digest binding, pre-stat time-of-check/time-of-use, hash-subprocess timeout, and the hash child's environment. The depth rows are the object, array, and mixed documents at measured depths 64, 65, and 20000, plus the scalar, escape, UTF-8, and duplicate-key spawns. That is the bound this run showed. The checker remains a record comparator. It is not a scanner, a legal owner, or a release signoff.
