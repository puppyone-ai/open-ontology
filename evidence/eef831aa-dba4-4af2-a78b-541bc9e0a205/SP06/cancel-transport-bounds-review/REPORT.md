# a205 report — fixed cancellation transport bounds review

Task `a205`. Session `eef831aa-dba4-4af2-a78b-541bc9e0a205`. Repository `/Users/redux/puppy-one/open-ontology`.

Verdict: **REQUEST_CHANGES**. The counterexamples and source lines are in `FINDINGS.md`. This review does not approve a release and does not mark a parent item done.

Fixed source `0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1`. Parent `39d2851cd70b2591768d85025550dde0924bf81a`. Tree `358029744eb8755834953fdda7bf6cfaf44edf9e`. Author evidence `c989ccf50b54e007d4a8ce118d7a624038a77ba9` (a196). Prior review evidence `65169f374c665d8efdd62a8bcd47317c38a1359e` (a191, APPROVE LIMITED on the earlier shutdown commit). No new source commit was created. Product sources, manifests, locks, gates, and ledgers were left untouched.

The review read the three a196 reports, the a191 findings on the unbounded `cancel_query`, `contracts/gateway/query-executor-v1.md` section "Cancel, failure, reuse", and `docs/17-rust-implementation.md` section 2. No `AGENTS.md` is present. The archive is `git archive` of `0ee7cf86` under `/tmp`, not a branch move. The review harness is `overlay/query_executor_pg_live.rs` mounted only into the proof and clippy builders.

## What failed

Aborting `CancelHandle::cancel` after `try_reserve_cancel` leaves `cancel_reserved` set. The producer then exits and `open` becomes false, and the next `execute_trusted_select` is still `SessionBusy` after 1200 ms. A retry cancel is `NotSent` and does not clear it. Dropping the in-flight stream after the same abort returns in 727 ms on the shared deadline, with rollback not confirmed and the backend row still present, and the next call is `Busy`. Details, oracle lines, and the repair shape are in `FINDINGS.md` sections 1, 2, and 5.

## What held on the same run

A stall of only the cancel transport, with the future left to finish, returned in 752 ms (`stalled_drop`, `fresh_deadline_drop`). Status `partial`, SQLSTATE absent, rollback not claimed, backend row present, SLO miss, next call `SessionClosed`. The handle's own timeout returned in 754 ms, `SessionBusy` while the reservation stood, then `SessionClosed`. Two successful queries on that session left it open, and the following cleanup still had a fresh 750 ms budget. Releasing the held cancel bytes left the next pid active. Healthy drops confirmed rollback and, where the oracle records it, backend absence (`healthy_after_reset` 47 ms, `backend_gone:true`; lock and sleep drops 6 ms). The whole-page client wall was 3044 ms with SQLSTATE absent. Server `57014`, user cancel `57014`, and client budget `57014` stayed distinct from the drop summaries. Role reuse was refused, an invalid role did not bind, and the flag-role path was `SessionClosed`.

## Archive and smoke

Pre-overlay smoke, before the review test was mounted:

- Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux`, `arch=arm64`. `linux/smoke/image.txt`.
- Container `oo-a205-smoke`, id `22dba3e8e09b162e4e6743aba6f7ea08485a559934ba1bce65a16a00baf47486`. `linux/arm64`, `--pull never`, `--cap-drop ALL`, `CapAdd` null, privileged false, memory 8589934592, `PortBindings` empty, network `oo-a205-net`. `OOMKilled=false`. `linux/smoke/hostconfig.txt`.
- `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `RUSTUP_TOOLCHAIN=1.94.0`.
- `linux/smoke/smoke.exit` is 0. `docker-run.exit` is 0. Console ends `smoke_phases_ok`. Library tests 17 passed, 0 failed. Clippy `-D warnings` on `--lib --test query_executor_pg_live` exited 0. `overlay-absent.txt` is `fixed=0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1 overlay=absent`.
- `linux/smoke/owned-sha256.txt` matches `source/blob-sha256.txt` for `query_executor.rs`, the author `query_executor_pg_live.rs`, and `query-executor-v1.md`.
- `linux/smoke/unsafe-lines.txt` is empty. `phases/unsafe.exit` is 0.
- Capacity at smoke start: `host_cargo=0 rust_builders=1 global=1 cap=6` (one other owner's builder was already up). This task ran one builder.
- Preserved misses: `linux/smoke/attempt-1/` (curl exit 52 from deb.debian.org), `linux/smoke/attempt-2/` (`libclang.so` present from a partial extract, `libLLVM.so.19.1` absent). Both `why-stopped.txt` files say this is not a product failure.

## PostgreSQL

Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `pg/version.txt` is `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`. Container `oo-a205-pg`, id `cabbfc900f784b75caa80521eb20a54ac0c8aa581d308e4919ee38fd1c70bb77`. `pg/up.exit` is 0. `pg/inspect.txt` records status running, exit 0, oom false, privileged false, network `oo-a205-net`. `pg/ports.json` is `{}`. `pg/caps.json` is drop `ALL`, add `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. Memory 1g, 1 cpu. Aliases `oo-a205-pg` and `oo-a189-pg`. SAN: `DNS:oo-a189-pg, DNS:oo-a205-pg, DNS:oo-a196-proxy, DNS:oo-a205-proxy`. `server.key` stayed under `/tmp` and was not mounted into a Rust builder.

## Proof

One proof builder after smoke, same Rust image and archive, overlay mounted read-only over the author test file.

- Container `oo-a205-proof`, id `9eb13e6b331b01beec88c16bc2370070bfeb9d08578f56e9b0dd0c6c023c2d13`. Privileged false, `CapDrop=["ALL"]`, memory 8589934592, `PortBindings` empty, network `oo-a205-net`, `OOMKilled=false`. `linux/proof/hostconfig.txt`.
- `linux/proof/phases/lib-test.exit` is 0 (17 passed). `phases/live.exit` is 0. `live.log`: `test live_permission_bound_select ... ok`, finished in 25.37s. `phases/unsafe.exit` is 0. `overlay-unsafe.txt` is empty.
- `linux/proof/proof.exit` is 2 and `docker-run.exit` is 2. Console line is `clippy component missing`, `phase_exit=2 phase=clippy-component`. Smoke had installed clippy into `HOME=/tmp`, which does not survive into the next container. The live oracle was already copied to `linux/proof/oracle.txt` and `live/oracle.txt` before that exit. This is not a product failure and not a live failure.
- `linux/proof/owned-sha256.txt` matches the executor and contract blobs. The test-file digest there is the overlay (`9859f55c63ca925b529c0dbb5d0169dad9927432bdb73c684c3e8b64928ff012`), not the author blob.
- Capacity at proof start: `host_cargo=0 rust_builders=0 global=0 cap=6`.
- Preserved miss: `linux/proof/attempt-1/` (overlay did not compile: `read_exact` returns `usize`, tuple types differed, `settle_exec` borrowed the session twice). `proof.exit` in that attempt is 101. `why-stopped.txt` says this is not a product failure. The overlay was fixed and the live test was rerun. The rerun is the oracle above.

## Clippy follow-up

`oo-a205-clippy` ran the same archive and the same overlay bytes. It did not rerun the live test. `RUSTUP_HOME=/cargo-home/rustup`. No fixture secrets were mounted. `--cap-drop ALL`, memory 8g, no published ports, network `oo-a205-net`.

- `linux/proof/phases/clippy-component.exit` is 0. `phases/clippy.exit` is 0. `clippy-docker.exit` is 0.
- `clippy-hostconfig.txt`: `ExitCode=0 OOMKilled=false Privileged=false CapDrop=["ALL"] Memory=8589934592`.
- `clippy.log`: `Finished dev profile ... in 4.23s` for `oo-gateway` lib and the live test. `clippy-console.log` ends `clippy_exit=0`.
- Capacity: `host_cargo=0 rust_builders=0 global=0 cap=6`. The container was removed by the launch script (`clippy-removed.txt` is `oo-a205-clippy`).

## Blobs

`source/commit.txt`, `source/parent.txt`, and `source/tree.txt` name the fixed commit. `source/blobs/` holds the three files from that commit. Digests:

```text
2ad166e91a407f5bff39d814d7beb4a1bc69710e30851260528184f3e8558925  crates/oo-gateway/src/query_executor.rs
7d704e52fc23fe7cdafb20f9830f7e4017182affa9de8304c603fefbca32f0ca  crates/oo-gateway/tests/query_executor_pg_live.rs
83fc0172b1d5d03c5524ad2487e9d8015f26575e684f91b82e243787d1c5b255  contracts/gateway/query-executor-v1.md
```

`SHA256SUMS` is `shasum -a 256` of every other file in this directory. `SHA256SUMS.sha256` is the digest of that list. No host Python wrote those digests.

## Scope left untouched

No broad typed-AST or type-certificate campaign. No parent checkbox. No release. The fixture functions are volatile and are not an R4 whitelist. The audit hook remains a second client's autocommit insert. `UNTRUSTED_ADMISSION` stayed blocked (`untrusted_admission=blocked`).
