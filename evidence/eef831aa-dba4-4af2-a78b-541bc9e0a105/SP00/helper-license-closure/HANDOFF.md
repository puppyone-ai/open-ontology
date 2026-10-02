# Handoff — helper license evidence closure

Stop here for an independent fixed review. Do not set `audited` to true. Do not mark a parent checkbox. Do not treat this lane as AT-SEC-05, release signoff, or an owner appointment.

## What landed

- Five unaudited records: `testing/harness/containment-supervisor/provenance/audit-records/{bitflags-2.13.2,cfg-if-1.0.5,cfg_aliases-0.2.2,libc-0.2.189,nix-0.31.3}.json`.
- Canonical upstream bytes: `testing/harness/containment-supervisor/provenance/licenses/<package>-<version>/`. cfg_aliases `NOTICE` matches `NOTICES.md` byte for byte. See `PROVENANCE.md` in that directory.
- `provenance/README.md` describes the closure. Flat license files that were already there were not edited.
- Std-only verifier: `testing/harness/helper-evidence-check`. No registry dependencies. Own `Cargo.toml` and `Cargo.lock`.
- Evidence and command logs: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a105/SP00/helper-license-closure/`.

The helper runtime source, manifest, lock, and binary were not edited. Root Cargo, jobs, pipeline, CLI, supply-chain policy, `ci/deny.toml`, xtask, Spec, and Plan were not edited.

## Facts the reviewer can re-check

Adoption `2026-10-02T11:10:37Z`. Age floors: nix 144, libc 72, cfg_aliases 77, bitflags 22, cfg-if 15. All are at least 168 hours. cfg-if is the tightest.

License choices under the existing policy: libc, bitflags, and cfg-if are `MIT OR Apache-2.0` with `dual_license_choice` MIT, and both license texts stay. nix is MIT. cfg_aliases is MIT. No OR relaxation and no corporate legal signoff were added.

cfg_aliases MSRV is undeclared. The crates.io `rust_version` is null.

libc's Apache file has no appendix. The other two OR crates include the standard placeholder appendix line, which is not a named author.

Historical scan only: cargo-audit 0.22.2, cargo-deny 0.20.2, RustSec `117edb3bed98e9be112f277b7615eea3252e7c43`, commit time `2026-10-02T10:58:33+02:00`, recorded `2026-10-02T12:12:14Z`. Lock sha256 `14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1`. deny.toml sha256 `4320807bfe747fe339ce7bf539a1cff5604e8e0c7677760e2cbc7abc1868624b`.

## What passed

Linux arm64 Rust 1.94.0 container `5e7f04e8faefbe76e3255092710552402ef8aa3082feff0b59474f85fe89f7ad` on image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Readonly `git archive` of `f1a6005971e3dc3075094c87ed670fc9d25e9ae0`. Final exits: helper fetch, test, metadata, and `cargo tree -e features` were 0. Verifier fetch, test (25 closure tests plus 3 unit tests), and `cargo clippy --locked --offline -- -D warnings` were 0. Clippy was added once with `rustup component add clippy --toolchain 1.94.0` because that toolchain did not ship it. Details and earlier failed attempts are in `REPORT.md` and `raw/`.

`governance/check_governance.py --self-test` exited 0 and does not read these records. An in-memory `audited_record_errors` flip returned zero errors and was not written back. Disk `audited` is false.

## Still open

Named owner, security exception, protected CI, runsc, image, whole graph, secret rotation, release, full AT-SEC-05, and the platform C library reached through libc.

## Workspace SHAs

Named baseline `e33f108b8226a7e5a80fa184a9a531751d491aa2`.

At container launch `2026-10-02T13:45:06Z`: HEAD `3ca5357930a2c44c42d3061db5fabe577881269a`, tree `ec11cd3ee7948c33e6d43cd2e984e01b948f94e6`, parent `dd5ecf39604b8467a0d1f91d56d2e90473fa2c48`.

The commit that contains this handoff records its parent in the git object. Re-resolve `git rev-parse HEAD` after the commit; do not copy an older SHA forward.

Immediately before commit:

- HEAD `6cd955af38ce426bb1102a6e078e8580febed776`
- tree `463bbf6ada8ea58e676370c872bfd711e9b9c251`
- parent `3ca5357930a2c44c42d3061db5fabe577881269a`
