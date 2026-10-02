# a091 P5 IPC fixed review

Decision: approve the IPC fail-closed slice at `6831d6e862303a0ce1c883925068078cbcb78e23`. ADR `docs/adr/ADR-P05-parser-prototype.md` stays Proposed. This review does not sign P5, Gateway, AT-GW, query safety, R4, R5, R9, OID/ACL/RLS, network isolation, shared source access, runsc, native parser 17.7 versus server 17.10, amd64, or full supply.

The first container exit 1 is the independent probe failing to compile (`harness-build.exit` 101, closure lifetime). The product smoke under that same container is exit 0. A second probe run exited 1 because the evil image was exec'd as the supervisor and returned before any frame (`probe-retry.exit` 1, 14 `SPAWN_FAILED` rows). Those two exits are preserved. They are not the parser result. The conclusive probe is `probe-keep.exit` 0, summary `fails` 0.

## Identity

| Item | Value |
|---|---|
| Commit | `6831d6e862303a0ce1c883925068078cbcb78e23` |
| Tree | `2f7fc8e8bb8ea34551235190e8e200e750358af9` |
| Parent | `77d957750e18c7e811e845a7717d54372b41839e` |
| Subject | parser: fail closed on contradictory IPC and bound worker cleanup |
| Standalone lock SHA256 | `c46ce7f6a90e7f2983eaa4bb1471fb45e275e38a220716175801d56981ce53b7` |
| Corpus blob at commit and parent | `2ead1211bc596463550d4741862364fd45c589e4` |
| `corpus/expected.json` SHA256 | `6f35c87189ea79adc41399977237e7f6c29f5c001cc8d86e06632678db519931` |
| ADR blob | `9de08bb5e96555cf0fb39fbac476795b62404e9e528ffb12680d6c2a90a176b1` |
| Archive | 40 spike files, worktree diff versus the commit was 0 lines at archive time |
| Default worker SHA256 | `3fc67f567e1fdec86052ff077feb03cb54e5b94d23b64c74788bfef25b5ee44e` |

Smoke input was the git archive of that commit, mounted read-only at `/src`. The corpus file was not reblessed. No PostgreSQL process was started. `DROP TABLE`, `set_config`, and `pg_read_file` were classified in process or as worker output only.

## Smoke

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, local, `linux/arm64`, `--pull never`. Container `oo-a091-msrv-051d4062`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, cargo 1.94.0. `protoc` absent. `/proc/1` and `/proc/self` `CapEff=0000000000000000`, `NoNewPrivs=1`. HostConfig: `CapDrop=["ALL"]`, `Privileged=false`, `PortBindings={}`, network `oo-a091-051d4062-net`, tmpfs `/tmp` `rw,nosuid,nodev,noexec,size=1g`, `no-new-privileges`, memory 6g, cpus 4, pids-limit 2048. Libclang 19.1.7 extracted from the nine recorded arm64 debs. `ldd` on `libclang-19.so.19` had no `not found` (`ldd.exit` 0). One own builder. At launch, other running rust-builders were 0 and host `cargo`/`rustc`/`clippy-driver` were 0. The later probe reruns ran while `oo-a096` was the other builder.

Commands, all `--manifest-path /src/Cargo.toml`. Fetch had no `-p`.

| Phase | Exit |
|---|---|
| `cargo fetch --locked` | 0 |
| `cargo test --locked --offline` | 0 |
| `cargo clippy --locked --offline --all-targets -- -D warnings` | 0 |
| `cargo test --locked --offline --features test-hooks` | 0 |
| `cargo clippy --locked --offline --all-targets --features test-hooks -- -D warnings` | 0 |
| default `oo-p05-parser-worker --supervise 3 4 5 6 7 8 <hex nonce> /tmp allow-fault contradict` | 2 |
| harness attempt 1 | 101 |
| probe skipped | 127 |
| inside / docker-run | 1 |

Default tests: lib 10, corpus 4 in 81.33s (`native_matches_expected_corpus` ok), frame 7, doc 0. Hooks tests: lib 10, corpus 4 in 82.87s, frame 7, ipc 11 in 105.16s, doc 0. Author ipc names that passed include `false_allow_syntax_never_becomes_true`, `contradictory_shapes_clear_sql_and_never_allow`, `malformed_frames_finish_inside_the_cleanup_ceiling`, `held_stdout_samegroup_and_setsid_are_reaped`, `deadlines_are_separate_and_measured`, `foreign_sleep_survives_cleanup`. Clippy finished with `-D warnings` on both feature sets.

`allow-fault` on the default-feature worker exits 2 before `prctl` and before the inherited fds are used. The test-hooks feature is required for that fault path.

## Independent probe

`probe-keep.out`, exit 0, `fails` 0. `admit_in_worker` keeps an allow. `exercise_untrusted` forces `keep_allow` false, so it cannot by itself show a false+ALLOW promotion. Contradictory shapes were pinned through `admit_in_worker`. The evil image implements `--supervise` only so that same pinned image can fork its own worker; lifecycle of the Rust supervisor was measured with `exercise_untrusted` on `oo-p05-parser-worker`.

`syntax_allow false` + `ALLOW_SYNTAX` + `DROP TABLE analytics.orders` + version 170007 returned `INVALID_OUTPUT`, `syntax_allow false`, empty SQL, identity `pinned`, reaped true, wall 4ms. The same shape with version 170010 was `INVALID_OUTPUT`, not `PARSER_MISMATCH`. A consistent allow of that DROP (`syntax_allow true`, code `ALLOW_SYNTAX`, version 170007) returned `ALLOW_SYNTAX` and kept `DROP TABLE analytics.orders` (27 bytes). That call used the caller SHA-256 of the evil image. It was not sent to a database. The caller digest is not an external signature.

Also `INVALID_OUTPUT` with empty SQL, via `admit_in_worker`: garbage `{`, `oid_acl_rls_proven true`, unknown field `injected`, unknown code `DROP_DATABASE`, deny `NOT_SELECT` that still carried DROP SQL, `syntax_allow true` with code `UTILITY`, `syntax_allow false` with `ALLOW_SYNTAX` and empty SQL, oversize allow SQL (70000 bytes), and two frames in one write. Version 170010 with a consistent `ALLOW_SYNTAX` of `SELECT 1` was `PARSER_MISMATCH` with empty SQL.

In-process `admit_native("SELECT 1")` was `ALLOW_SYNTAX`. The pinned default worker returned the same SQL, `syntax_allow true`, `oid_acl_rls_proven false`, reaped true, identity `pinned`. `admit_native("DROP TABLE analytics.orders")` was `UTILITY` with empty SQL. `exercise_worker` on the hooks binary with `KIND_ADMIT` and `SELECT 1` returned `UNTRUSTED_WORKER`, empty SQL, identity `test-hooks-kind`. The parent did not repair that output by calling `admit_native`.

Open parser classifications, in process, not sent to a database: `set_config(...)` `ALLOW_SYNTAX`; `pg_read_file(...)` `ALLOW_SYNTAX`; `WHERE true` on `analytics.big_fact` deparsed to `SELECT b.id, b.ts ... WHERE true` and `ALLOW_SYNTAX`; a subquery star deparsed to explicit columns and `ALLOW_SYNTAX`; bare `SELECT * FROM analytics.big_fact` was `R9_STAR_ON_LARGE`.

## Clocks

Image copy sits outside the readiness and parse ceilings. `ready_wait_ms` starts after spawn.

| Case | wall ms | remainder ms | ready | parse | cleanup | code |
|---|---:|---:|---:|---:|---:|---|
| pinned `SELECT 1` | 1441 | 1438 | 1 | 1 | 1 | `ALLOW_SYNTAX` |
| bad magic | 2687 | 2670 | 0 | 0 | 17 | `INVALID_OUTPUT` |
| stdout junk | 2780 | | 0 | 0 | 13 | `INVALID_OUTPUT` |
| oversize frame | 2689 | | 0 | 0 | 12 | `RESPONSE_CAP` |
| partial frame | 3200 | 2668 | 0 | 507 | 25 | `DEADLINE` |
| silent before ready | 7726 | 2704 | 5003 | 0 | 19 | `SPAWN_FAILED` |
| early exit | 2795 | | 0 | 0 | 1 | `SPAWN_FAILED` |

Remainder on the hooks-sized supervisor is about 2.7s and matches a memfd copy. Silent readiness is 5003ms inside a 4900–5600 window. Partial parse is 507ms. Bad-frame cleanup is 12–25ms. None of those waits occupied the 5s readiness budget after the worker was already ready. During silent, a descendant exe was `/memfd:oo-p05-worker (deleted)`.

## After return

Proof lines are `PID <pid> <starttime> <ppid> <pgid> <sid>`. A starttime still present in `/proc` counts as alive.

- hold-setsid: `PID 339 286107 338 339 339` (pid = pgid = sid) and `FINAL 0`, reaped true, cleanup 17ms, proof alive 0. Foreign sleep survived.
- hold-same: `PID 343 286377 342 342 1` (pid differs from pgid and sid) and `FINAL 0`, reaped true, proof alive 0.
- success-hold through `admit_in_worker`: `ALLOW_SYNTAX` `SELECT 1`, `PID 347 ... 347 347`, `FINAL 0`, reaped true, cleanup 22ms, recorded starttime gone.
- success-same: `ALLOW_SYNTAX` `SELECT 1`, reaped true, same-group proof, recorded starttime gone.
- fork-race (setsid leader plus a loop of 20 children) and double-fork: reaped true, recorded starttimes gone (`recorded_alive` 0). The proof line listed the leader and seven of those children. This run cleared the pids it recorded. It does not prove every schedule, and it does not prove a pid namespace.
- Supervisor SIGKILL during `setsid-noready` (killed pid 396): code `SPAWN_FAILED`, `syntax_allow false`, empty SQL, reaped false, containment `not-proven`, ready_wait 5009ms. The setsid child 398 was still alive with ppid 1. The probe then killed only that pid because its starttime still matched. Foreign sleep survived. The parent did not authorize.
- `prctl` filter denying only `PR_SET_CHILD_SUBREAPER` (option 36) after `PR_SET_NO_NEW_PRIVS`: branch `executed`. Code `CONTAINMENT_UNAVAILABLE`, containment `unavailable`, reaped false, `syntax_allow false`, empty SQL, proof text `UNAVAILABLE`.

`RLIMIT_FSIZE` stopped one write at `wrote=262144`. That single file stayed inside the directory byte budget, so the decision stayed `RESOURCE_BUDGET` and reaped true. Two 200KiB files returned `RESOURCE_BUDGET` and reaped false. 1500 empty files (`files=1500`) stayed `NOT_SELECT` and reaped true: the directory budget is a sum of file lengths, and `cleanup_wait_ms` is stored before that walk and `remove_dir_all` (observed 1ms). A one-byte file created outside the workdir at `/tmp/oo-a091-probe/outside.bin` still existed after return and was then unlinked by the probe.

The worker boundary line was `netns=net:[4026532567] path=/usr/bin:/bin home=unset passwd=yes cargo=yes socket=yes`. The predicate required the netns string to equal the parent. The worker opened `/etc/passwd` and `/src/Cargo.toml` and closed them without copying contents, and it created and closed a TCP socket. Env clear and a temp cwd are not network or filesystem isolation.

ECHILD and EINTR were not injected. The archive's `reap_zombies` breaks the `waitpid` loop when the return is `<= 0`, which includes both. That source fact is not a measured universal reap. `unshare(CLONE_NEWUSER)`, cgroup writes, and `RLIMIT_NPROC` were not run. The supervisor still polls `/proc` on a short interval. No poll-only containment claim.

## Licenses

Re-fetched 2026-10-02. Byte-equal to the spike copies and, for protobuf-c and the Bison prefix, to the crates.io `pg_query-6.2.0` tree. The crate directory listing of `LICENSE`, `COPYING`, and `COPYRIGHT` was empty. `PG_VERSION_NUM` in that crate is 170007. `cargo audit` and `cargo deny` were not run. xxhash was not re-fetched. This is not full supply approval.

| Text | SHA256 |
|---|---|
| pg_query.rs MIT at `0e382742b36ee9592438c6791b91ce14f32f6233` | `39fac7e7daa7cf3173d96f9e8a167f980b93b90386b83c342de70db3bc7bbb68` |
| libpg_query BSD-3 at `7be1aed1f1f968a36cf541319f71e845850f0381` | `f9352c20940619a380a5e9f23cebe34d8f797cfbd747fc236a7a3e5fc8ef9b41` |
| PostgreSQL `REL_17_7` `COPYRIGHT` | `5ed3ce5c9373dff7f98b1fae7a6c7ccd98df7d734d46d24c1bcebf1240be8307` |
| vendored `protobuf-c.h` | `3c06432ca3233a730e7b20afc1bc5750dd3f8c298db9c99b5df03dc6e1a4740c` |
| first 94 lines of `src_backend_parser_gram.c` | `e8ecf341ce94fb7ad19de4a4985374eb821ff84fd4dff943b10a022ca93fc7a3` |

The protobuf-c copyright line is 2008–2018. The spike excerpt `142a052f7d22404fb513ad9f8d03f78f27e797352292c3a60c5c2babcef8285e` is a prefix of that header. The Bison 2.3 text is GPL-2.0-or-later with the special exception for a larger work that is not itself a parser generator built from the skeleton. That exception is not a blanket GPL grant.

GitHub commit `7be1aed1f1f968a36cf541319f71e845850f0381`: message `Release 17-6.2.2 (#326)`, author Lukas Fittl, author date `2026-01-27T05:39:40Z`, `verification.verified=true`, reason `valid`.

## Root owner correction

`evidence/.../a080/SP01/P5-ipc-correction/HANDOFF.md` says a073 still owns the main root. That sentence is stale. This review does not edit that file and does not judge a073, a088, or a095. The ledger after a080 records a073 releasing the root, a088 taking and then releasing the NFC root, and the root moving on. For this resume the named writers are a095 for the CLI root, a098 for connector types after a093 exited, and a092 for pipeline. a090, a094, a096, and a097 are separate read-only reviews. No root lock, manifest, or foreign tree was written.

## Unsigned

Ready-frame nonce, pid, and starttime spoof was not an independent probe row. The evil image wrote the live session nonce and its own pid and starttime, so `contradict` reached the false+ALLOW check (`detail` `contradict`) only after those fields matched. Author test `spoofed_identity_is_rejected` in `raw/test-hooks.out` passed for `spoof-pid`, `spoof-start`, and `spoof-nonce` through `exercise_untrusted`: `INVALID_OUTPUT`, empty SQL, `starttime_matched` false, reaped. `accept_worker_decision` clears a worker-supplied `starttime_matched` before `enforce_parent`. The independent image did not send a decision body with a wrong nonce or pid.

R4 and R9 remain open on the classifications above. R5 was not exercised. Network and source-tree access remain as measured in the boundary line. runsc/gVisor was not used. The native parser is 17.7 (`170007`); server 17.10 was not started and `gram.y` was not diffed. amd64 was not run. Gateway bind and authorize were not run. OID, ACL, and RLS were not proved; a worker claim of `oid_acl_rls_proven true` was cleared. Full supply, `cargo audit`, and `cargo deny` were not approved. ECHILD injection, user namespaces, and cgroup enforcement were not run.

## Cleanup

Own resources only. No `docker prune`. Exits: msrv container 0, probe container 0, probe2 container 0, network 0, cargo volume 0, target volume 0, temp dir 0. `oo-a096-rust194-075afd80-ece4-4e25-bb21-21cdf591841f` was still running afterward and was left alone. Cleanup exits are separate from the smoke and probe exits.
