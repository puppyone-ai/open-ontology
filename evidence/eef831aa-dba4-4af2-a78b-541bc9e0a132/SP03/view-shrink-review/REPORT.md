# REPORT — a132 receive-view shrink review

**APPROVE LIMITED.** The panic removal and the meter adjustment are sound on the checks below. The production stream integration gate stays closed. A passing 128-frame test is not that gate. The 60000-row catalog path and a TLS production read stay pending. a131 keeps developing on the frozen baseline until a later review opens this gate.

| | |
| --- | --- |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a132` |
| Reviewed commit | `79515fcf7e7032c8c212540eaae4b48a5957e3a2` |
| Reviewed tree | `b78f84497484f6232aecf0b51ffcc6d27be9fd7c` |
| Reviewed parent | `3f66773adf1afe378fc73a8ca56abf25bcb556cf` |
| Prior evidence | `2b68ab3071f3e575535f207df8d7ca31b8625da7` (a129) |
| Image | `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` |
| OS / arch | linux / arm64 |
| Toolchain | rustc 1.94.0 (`4a4ef493e` 2026-03-02), cargo 1.94.0 (`85eff7c80` 2026-01-15) |
| Override | `RUSTUP_TOOLCHAIN=1.94.0` (`rust-toolchain.toml` still says 1.98.1) |
| PostgreSQL | none |

No product, manifest, lock, spec, or ledger edit. The init_mut proof is an overlay test copied into `/tmp/sqlx-core-pkg` inside the container. `buffered.rs` in that copy matched the archive before the pin, after the pin, and after the test (`buffered-copy-before.txt`, `buffered-copy-after.txt`).

## What was executed

Workspace tests used `cargo test --locked --offline` and `cargo clippy --locked --offline -p oo-connectors --all-targets -- -D warnings` against a `git archive` of `79515fcf`. One builder at a time, `--cap-drop ALL`, `no-new-privileges`, memory 8g, swap 8g, cpus 4, pids 4096, bridge `oo-a132-eef831aa-net`, `HOME=/tmp`, `--pull never`. The cgroup was 8g. The probe was 4 socket chunks. The 512-frame run was not repeated. `OOMKilled=false` on every container.

| Run | Result |
| --- | --- |
| First `lib` / `clippy` | 101 / 101. The archive omitted `contracts/source/pgsql/empty-catalog.json`. `model.rs:1044` includes it in a lib test. See `smoke-rerun-reason.txt`. |
| `smoke-rerun` lib | 0. 55 passed, 0 failed, 0.01s. Includes `empty_catalog_fixture_matches_the_frozen_hash`. |
| `clippy-rerun` | 0. `cargo clippy -p oo-connectors --all-targets -- -D warnings`. A login-shell attempt exited 127 because `bash -lc` dropped `cargo` from `PATH`; `clippy-rerun-login-miss.log` keeps that failure. The non-login rerun is the result. |
| `pg_wire_bounds` | 0. 13 passed, 6.06s. |
| `pg_wire_accounting` | 0. 12 passed, 4.59s. |
| `pg_portal_bounds` | 0. 6 passed, 26.25s. |
| Receive-cap probe | 0. 1 passed, 0.00s. `OOMKilled=false`. |
| First container `docker-run.exit` | 101, because the first lib and clippy exits were 101. `OOMKilled=false`. |

`cargo test -p sqlx-core` is not a workspace command: sqlx-core is vendored, not a workspace member. The probe ran as `cargo test --manifest-path /tmp/sqlx-core-pkg/Cargo.toml --test a132_receive_cap`. The copy's own lock named bytes 1.11.1 (`bytes-lock-before.txt`). The pin `version = "=1.12.1"` moved that lock to bytes 1.12.1 (`bytes-lock-after.txt`), which is the workspace `Cargo.lock` version. These numbers are that pin. This review did not execute the author's 128-frame `#[cfg(test)]` module, and it did not execute a 512-frame run.

Product blobs from the archive, sha256:

| File | SHA-256 |
| --- | --- |
| `buffered.rs` | `33d460f5005050a9cc3db6f90b022ef51b872e57a7552eec5103b31e10f0e1ba` |
| `PATCH.md` | `ef04fa1fec87e47eff8396e0dce91fa00a726c9bc9fc9bb64483816f314a8b60` |
| `pg_wire_view_shrink.rs` | `9b81825f1a21e0e581f31d9bedb62d1b52f420e9b60dc3983e1cc263efb86268` |

Those three match a129's GREEN hashes. The RED panic expect is absent.

## Gate

The limited approval covers `ReadBuffer::read` and `note_reserve` in `buffered.rs`: a falling view sum no longer panics, a completed read keeps the net increase of that step, and a parked cancel or EOF keeps the charge recorded before the read.

The integration gate stays closed. On bytes 1.12.1, four `init_mut` reads of 252 bytes raised the live view from 8192 to 32643 with the frames dropped, and to 40706 with the frames retained. The same four reads with an opt-in message cap of 32 produced the same view peak, 32643. An append-shaped reader, the Tokio plaintext contract, stayed at 8192. The cap does stop a declared oversize header: length 100000 was rejected at logical need 5 and the view stayed 8192.

Opening the gate takes a product change that discards the `init_mut` tail, then a new bounded proof that repeated short `init_mut` reads keep the live view from doubling, with the cap set, for both a dropped frame and a retained frame. The embedded 128-frame test is not that proof. No PostgreSQL catalog read was run here.
