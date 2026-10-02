# Helper license evidence closure

Session `eef831aa-dba4-4af2-a78b-541bc9e0a105`. Foundation lane for the containment-supervisor registry dependencies. This is not a product-progress metric. `audited` stays false. This report is not a release signoff, an owner appointment, or a completed AT-SEC-05 case.

## Workspace

Named baseline: `e33f108b8226a7e5a80fa184a9a531751d491aa2`.

Container launch at `2026-10-02T13:45:06Z`:

| | SHA |
| --- | --- |
| HEAD | `3ca5357930a2c44c42d3061db5fabe577881269a` |
| tree | `ec11cd3ee7948c33e6d43cd2e984e01b948f94e6` |
| parent | `dd5ecf39604b8467a0d1f91d56d2e90473fa2c48` |

HEAD immediately before the local commit is recorded in `HANDOFF.md` after `git rev-parse`. Other writers moved the shared workspace during this lane. The helper lock pin did not change.

Fixed helper archive input: commit `f1a6005971e3dc3075094c87ed670fc9d25e9ae0`, tree `86c64f512d67da47ae379a53e34342de8e317f25`, parent `7143b2d9a7888d242983e5b9c10ef597246d5848`. The smoke compiled that commit from `git archive`, mounted read-only. The moving workspace was not edited to make the helper build.

## Pins

| Input | sha256 |
| --- | --- |
| `testing/harness/containment-supervisor/Cargo.lock` | `14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1` |
| `ci/deny.toml` | `4320807bfe747fe339ce7bf539a1cff5604e8e0c7677760e2cbc7abc1868624b` |
| a101 `scan-audit.out` | `f64e43716f72c173879cb30130f73df0f1c802ac32960eb6c3955c15c2209f81` |
| a101 `scan-deny-advisories.err` | `02d186150402cda84d64b50da1dea4b586274b3afbd3fbaf078a7c4e389920c3` |
| a101 `age.txt` | `8ace2a347494e1767d9a9ac7d771f6e806aacba052fa2b55da920bef276d5160` |
| a101 `helper-metadata.out` | `6d1a3456f8781bc18989553ea9362109c39c97aff480636a7251bc28cab60186` |
| a101 `advisory-rev.txt` | `ee376fcdd344d86d428ff4f80997e660fe2b2217fdf77139ddfcb5b6e3055cbd` |

The archive lock and the workspace lock both hashed to the lock pin inside the container (`raw/archive-lock.sha256`, `raw/repo-lock.sha256`).

Adoption instant is the helper lock commit time `2026-10-02T11:10:37Z`. Publication times are crates.io `version.created_at`. The publisher `created_at` and the crate-object `created_at` are not the age input. `MISSING_CRATE` is not a dependency.

Retained scan: cargo-audit 0.22.2 and cargo-deny 0.20.2, RustSec `117edb3bed98e9be112f277b7615eea3252e7c43` at `2026-10-02T10:58:33+02:00`, recorded at `2026-10-02T12:12:14Z`. That snapshot is historical. It was not rerun. The tools were not rebuilt.

## Records

Five files in `testing/harness/containment-supervisor/provenance/audit-records/`. On disk: `audited` false, `template` false, `admission` false, `release_signoff` false, `full_at_sec_05` false, `license_record_status` `complete-for-review`, `do_not_treat_as_baseline` true, `seven_day_rule` `pass`, `exception` null, `reviewers_assigned` empty.

| Package | Expression | Choice | Published (version.created_at) | Age floor (days) | Role |
| --- | --- | --- | --- | --- | --- |
| nix 0.31.3 | MIT | null | `2026-05-11T07:01:18.650221Z` | 144 | direct, feature `signal`, defaults off |
| libc 0.2.189 | `MIT OR Apache-2.0` | MIT | `2026-07-21T21:33:28.018110Z` | 72 | normal transitive, features `default`, `extra_traits`, `std` |
| cfg_aliases 0.2.2 | MIT | null | `2026-07-16T14:57:16.699789Z` | 77 | build dependency of nix |
| bitflags 2.13.2 | `MIT OR Apache-2.0` | MIT | `2026-09-10T01:11:38.500809Z` | 22 | normal transitive, feature `default` |
| cfg-if 1.0.5 | `MIT OR Apache-2.0` | MIT | `2026-09-16T17:36:11.278420Z` | 15 | normal transitive, feature `default` |

cfg_aliases has no `rust_version` on the crates.io version object. The record MSRV is the sentence `not declared in the published manifest or the crates.io version object; edition 2018`. Edition 2018 is the published manifest fact. No numeric MSRV was invented.

OR crates keep both MIT and Apache-2.0 files and obligations. The MIT choice does not drop the Apache evidence. nix is MIT only. cfg_aliases is MIT only.

cfg_aliases keeps upstream `NOTICES.md` and a byte-identical `NOTICE` (`1e2b7ade3fb228130408b9990cae6a7618eb314c75aa0b164bfe485d9d9756ee`). `NOTICE` is the locator. The copy does not change the Katharos copyright or the `tectonic_cfg_support` MIT notice. See `licenses/cfg_aliases-0.2.2/PROVENANCE.md`. Flat license files already in `provenance/licenses/` were not modified.

libc `LICENSE-APACHE` is the upstream 9723-byte text. It ends at `END OF TERMS AND CONDITIONS` and has no appendix. `apache_appendix_copyright` is null on that record. bitflags and cfg-if ship the appendix line `Copyright [yyyy] [name of copyright owner]`, which is the standard placeholder and not a named owner.

libc `native_link` is `ffi-bindings-to-platform-c-library`. The platform C library license was not scanned. `open_gates.platform_c_library` is `open`. No cc, bindgen, or cmake crate is in this graph.

The helper manifest license is `Apache-2.0`. That first-party license is not one of the five records.

Open on every record: named owner, security exception, protected CI, runsc, image, whole graph, secret rotation, release, full AT-SEC-05, platform C library.

## Verifier

`testing/harness/helper-evidence-check` is a std-only package with its own `[workspace]`, no dependencies, and `forbid(unsafe_code)`. It checks the five records against the inventory, the helper lock, `ci/deny.toml`, the helper manifest line `license = "Apache-2.0"`, the retained a101 bytes, and the canonical license files. Digests come from `/usr/bin/sha256sum` or `/usr/bin/shasum -a 256`. It does not read environment variables. Exit 0 prints `audited=false` and `release_signoff=false`.

An in-memory call of `governance/qa_record.py` `audited_record_errors` at `2026-10-02T12:12:14Z`, with `audited` flipped only in process memory, returned zero errors for each of the five records (`raw/governance-record-shape.out`). The files on disk were not flipped. `governance/check_governance.py --self-test` exited 0 (`applicable_passed=120`). That self-test does not read these records and does not pass AT-SEC-05. The Rust verifier is the artifact check. The record checker is not a security scanner. Record eligibility is separate from release signoff.

## Commands and exits

One container, sequential phases. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `--platform linux/arm64`, `--pull never`. Container id `5e7f04e8faefbe76e3255092710552402ef8aa3082feff0b59474f85fe89f7ad`, name `oo-a105-helper-evidence-20aeb9b6-8cd5-4375-8920-c39c2d2bb7a6`. `--cap-drop ALL`, `no-new-privileges`, memory 8g, cpus 4, pids 4096, private bridge network, private cargo and target volumes, `HOME=/work/home`, `/tmp` tmpfs. No host network, docker socket, host HOME, or shared cargo cache. Builder count before launch: host cargo/rustc/rustup 0, rust containers 0.

`rustc 1.94.0 (4a4ef493e 2026-03-02)` with `RUSTUP_TOOLCHAIN=1.94.0`. The named toolchain did not already contain clippy. `rustup component add clippy --toolchain 1.94.0` exited 0, then clippy ran.

| Phase | Exit |
| --- | --- |
| `cargo fetch --locked` helper manifest | 0 |
| `cargo test --locked --offline` helper | 0 (lib 3, branches 8, main 0, doctests 0) |
| `cargo metadata --locked --offline --format-version 1` helper | 0 |
| `cargo tree --locked --offline -e features` helper | 0 |
| `cargo fetch --locked` verifier | 0 |
| `cargo test --locked --offline` verifier | 0 (lib 3, closure 25, main 0, doctests 0) |
| `cargo clippy --locked --offline -- -D warnings` verifier | 0 |
| `python3 governance/check_governance.py --self-test` | 0 |

The feature tree shows nix `signal` and `process`, libc `default`, `std`, and `extra_traits`, and cfg_aliases as a build dependency. Logs are in `raw/`.

Closure tests that exited 0 on the real tree: the five-record positive case and usage. Negatives, each on a `/tmp/oo-a105-helper-evidence-*` copy, exited with the expected code: missing license body 2, license byte mutation 3, deny.toml hash 3, wrong dual choice 4, undeclared choice 5, omitted Apache file 6, missing cfg_aliases NOTICE 7, publisher timestamp `2021-12-30T16:00:11Z` 8, age floor 9, changed helper lock 10, missing audit output 11, `..` source 12, CLI `..` 12, symlink license 12, unknown package 13, duplicate record 14, wrong repository 15, expression `ISC` 16, obligation `n/a` 17, record digest 18, `audited: true` 19, `fresh_current_scan: true` 19, graph role 20. Failure text did not include license bodies.

## Retained failed attempts

Earlier containers were removed before the next one started. Their logs are under `raw/attempt-1/`, `raw/attempt-2-verifier-test.err`, `raw/attempt-3-verifier-test.err`, `raw/attempt-4-verifier-test.out`, and `raw/attempt-5/`.

1. Verifier did not compile: `String` versus `&str` in the license-source checks, and `BTreeSet<String>::contains`.
2. `env!("CARGO_BIN_EXE_...")` was unset for this package. Tests locate the binary from `current_exe`.
3. The Apache check required the appendix placeholder. libc's upstream file has no appendix, so every record failed `omitted-apache-body` before later nix checks. The check now requires `Apache License`, `Version 2.0`, and `END OF TERMS AND CONDITIONS`.
4. Clippy `-D warnings` failed on `nonminimal_bool`, `while_let_on_iterator`, and `collapsible_if` after the 25 tests had passed. Those three lints were fixed. The final clippy exit is 0.

## Cleanup

Removed container `oo-a105-helper-evidence-20aeb9b6-8cd5-4375-8920-c39c2d2bb7a6`, network `oo-a105-20aeb9b6-8cd5-4375-8920-c39c2d2bb7a6-net`, and volumes `oo-a105-20aeb9b6-8cd5-4375-8920-c39c2d2bb7a6-cargo` and `oo-a105-20aeb9b6-8cd5-4375-8920-c39c2d2bb7a6-target`. Removed `/tmp/oo-a105-20aeb9b6-8cd5-4375-8920-c39c2d2bb7a6` and the probe files from a short image PATH check. No `docker prune`. No other container was named in the cleanup commands. `docker ps` before and after this launch were empty captures.

## Source hashes

```
09a5043dbfb5f50bc7ae9425a907cb3cd61407c15fe76f2412eec5bf4bcabb75  testing/harness/helper-evidence-check/Cargo.lock
bf4fd8debabc3ab9d5d5a57dbe2712efb9f647dd59b035a33a763fdf32ae3700  testing/harness/helper-evidence-check/Cargo.toml
fbd5146701bb52272eaae030d3874aef9fca6196468f09d65bd9493c84275844  testing/harness/helper-evidence-check/fixtures/inventory.json
3df686a8085f765253441dcd7af095c0d1941371a12afbf35e6c0c9c7b67c3ef  testing/harness/helper-evidence-check/src/check.rs
e6155f511a7ea922fedb04de1496af05b77079643071bbf6427ffc6061f96090  testing/harness/helper-evidence-check/src/fail.rs
bef325c141952b92d584a9f99fadc357207f2972fe046ae2385e9ebab89d29ce  testing/harness/helper-evidence-check/src/json.rs
b4440e301fd2f5f8faa361a1fcc8578ccb1a9a6cad5a996fc501680b8a002866  testing/harness/helper-evidence-check/src/lib.rs
cef288c014d525c043db77cf2dc80e4e37bcdec747921f9d0ea84af0aa3cdc8d  testing/harness/helper-evidence-check/src/main.rs
661e210119752de40b37bbf958a64e8bb5f84c37c2f3656aee899ebd3ac9b532  testing/harness/helper-evidence-check/src/sha.rs
5423f8ea6e9786b6c54b458abdf6498de5d78d76c124e5c41ea4d86c54a9ec1a  testing/harness/helper-evidence-check/src/timeutil.rs
2357326cdcde458bfe7219bbc930141d4bde15511968406a3e0e15c54aace4db  testing/harness/helper-evidence-check/tests/closure.rs
d439576a82d931be681532afab928d3f08d884845982efe9ca8f80f03a20977f  testing/harness/containment-supervisor/provenance/audit-records/bitflags-2.13.2.json
183de8fc202fd80fc19ac3b7dbf82563406d91fed41b090be8eef97612255dc4  testing/harness/containment-supervisor/provenance/audit-records/cfg-if-1.0.5.json
673847c4d429c2347db5c667bcbb0665549fe2b56fb15918f131712f8c57aabc  testing/harness/containment-supervisor/provenance/audit-records/cfg_aliases-0.2.2.json
3c27783c2e435a7c6c86a2a037dd8aba86161e7f54a24d1bcb9ad80210548218  testing/harness/containment-supervisor/provenance/audit-records/libc-0.2.189.json
242bb25b83a77edb8535e29d027fe94d18119a076525ce8808ddf11bf5a24367  testing/harness/containment-supervisor/provenance/audit-records/nix-0.31.3.json
a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2  testing/harness/containment-supervisor/provenance/licenses/bitflags-2.13.2/LICENSE-APACHE
6485b8ed310d3f0340bf1ad1f47645069ce4069dcc6bb46c7d5c6faf41de1fdb  testing/harness/containment-supervisor/provenance/licenses/bitflags-2.13.2/LICENSE-MIT
a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2  testing/harness/containment-supervisor/provenance/licenses/cfg-if-1.0.5/LICENSE-APACHE
378f5840b258e2779c39418f3f2d7b2ba96f1c7917dd6be0713f88305dbda397  testing/harness/containment-supervisor/provenance/licenses/cfg-if-1.0.5/LICENSE-MIT
31b94860253d8ec7b4529f51901044d3b459d6292d996504a36b1bae3a36a812  testing/harness/containment-supervisor/provenance/licenses/cfg_aliases-0.2.2/LICENSE
1e2b7ade3fb228130408b9990cae6a7618eb314c75aa0b164bfe485d9d9756ee  testing/harness/containment-supervisor/provenance/licenses/cfg_aliases-0.2.2/NOTICE
1e2b7ade3fb228130408b9990cae6a7618eb314c75aa0b164bfe485d9d9756ee  testing/harness/containment-supervisor/provenance/licenses/cfg_aliases-0.2.2/NOTICES.md
18186f3cde9e1e18d3cb8e03edd8a364bc1e02b2cc5534f05d54ab0411c893df  testing/harness/containment-supervisor/provenance/licenses/cfg_aliases-0.2.2/PROVENANCE.md
62c7a1e35f56406896d7aa7ca52d0cc0d272ac022b5d2796e7d6905db8a3636a  testing/harness/containment-supervisor/provenance/licenses/libc-0.2.189/LICENSE-APACHE
123a331b5dbf04c30097fa43b8f858bc85df671fe776de498d01f3d6b7c1f69e  testing/harness/containment-supervisor/provenance/licenses/libc-0.2.189/LICENSE-MIT
66e3ee1fa7f909ad3c612d556f2a0cdabcd809ad6e66f3b0605015ac64841b70  testing/harness/containment-supervisor/provenance/licenses/nix-0.31.3/LICENSE
5bf017bd501568b59ab1c4e0a49f1127866045fbdd4498685760e9e15d9ff427  testing/harness/containment-supervisor/provenance/README.md
```

## Residuals

Independent review has not happened. `audited` is false. Named owners are not appointed. No security exception exists. Protected CI, runsc, image, whole-graph review, secret rotation, and release remain open. The platform C library behind libc remains open. The retained advisory output is not a fresh scan. Full AT-SEC-05 remains open. This lane does not approve itself.

Immediately before commit: HEAD `6cd955af38ce426bb1102a6e078e8580febed776`, tree `463bbf6ada8ea58e676370c872bfd711e9b9c251`, parent `3ca5357930a2c44c42d3061db5fabe577881269a`.
