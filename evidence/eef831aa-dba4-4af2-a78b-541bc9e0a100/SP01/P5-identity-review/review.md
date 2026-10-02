# a100 independent IPC identity supplement

This file supplements a091. It leaves `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a091/SP01/P5-ipc-review/review.md` and `raw/probe-keep.out` unchanged. a091 approved a limited IPC fail-closed slice and recorded that the independent nonce, pid, and starttime spoof was still an author `test-hooks` test. This run is that missing production-parent probe.

Decision: `identity_checks_held` for the identity cases below. The ADR stays Proposed. This supplement does not sign P5, R4, R5, R9, Gateway, OID/ACL/RLS authorization, native 17.7 versus server 17.10, network, source I/O, pid namespace, runsc, supply, amd64, or release. A consistent caller-pinned `DROP` can still be a syntactic allow. This probe did not send `DROP`.

No product tree, root manifest, lock, spike, ADR, or older evidence was edited. No golden was reblessed. No SQL was sent to PostgreSQL.

## Fixed subject

| Item | Value |
| --- | --- |
| Commit | `6831d6e862303a0ce1c883925068078cbcb78e23` |
| Tree | `2f7fc8e8bb8ea34551235190e8e200e750358af9` |
| Parent | `77d957750e18c7e811e845a7717d54372b41839e` |
| ADR | `docs/adr/ADR-P05-parser-prototype.md`, status Proposed, SHA-256 `9de08bb5e96555cf0fb39fbac476795b62404e9e528ffb12680d6c2a90a176b1` |
| Spike lock | `c46ce7f6a90e7f2983eaa4bb1471fb45e275e38a220716175801d56981ce53b7` |
| `corpus/expected.json` | SHA-256 `6f35c87189ea79adc41399977237e7f6c29f5c001cc8d86e06632678db519931`, git blob `2ead1211bc596463550d4741862364fd45c589e4` |
| Archive tar | `7dc4fb2fcafdeb2f11a634935decb560d83d52737bec1092cd02fa71cc338fcd` |
| a091 `probe-keep.out` | `fb6984490f9b1fa89945e21665e6194475d239ebba4f7e86ead3953f4a651a46` |

The probe linked the archive at `/src` with `default-features = false`. The crate default feature set is empty, so `test-hooks` stayed off. Corpus, default tests, hooks tests, and Clippy were left as a091 recorded them.

Session names at this handoff: a098 is the root and connector writer, a102 is the jobs migration, a103 is the CLI read-only review. This file does not write those trees.

## How the probe called the parent

`raw/probes/src/main.rs` is `#![forbid(unsafe_code)]`. `--supervise` calls `oo_p05_pg17_parser::run_worker()`, so the pinned image has the supervisor entry a091's first evil image lacked. A 32-hex argument is this probe's worker. `--probe` is the harness.

Each identity case calls production `admit_in_worker` with the probe binary, `sha256_file` of that binary, SQL `SELECT 1`, and `/src/fixtures/policy.json`. The image record says `caller_hash_is_signature: false` and `test_hooks: false`. The pin is a SHA-256 of the image bytes.

`KIND_READY` (1) and `KIND_RESPONSE` (3) are crate-private in the fixed `lib.rs`. The probe carries those numbers locally. `KIND_ADMIT` (2) is public and is the kind the worker observed.

The control decision is distinctive: `syntax_allow` true, code `ALLOW_SYNTAX`, `parser_version` 170007, `node_count` 7, `max_depth` 1, `executed_sql` `SELECT 1`, `detail` `a100-control`. One in-process `admit_native("SELECT 1")` returned the same code and SQL with `detail` empty and `node_count` 3. That call did not open a database. A parent that repaired the reply through the native parser would have lost `a100-control` and node count 7.

The foreign process is the harness's own `/bin/sleep 240`: pid 1852, ppid 1851, starttime `548271`. The spoofed pid is 99999. The note on every spoof records `absent_existed` false and `absent_is_foreign` false. 99999 is not 1852.

## Commands, toolchain, exits

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. Container `oo-a100-msrv-5dd178d8`, network `oo-a100-5dd178d8-net`, volumes `oo-a100-5dd178d8-cargo` and `oo-a100-5dd178d8-target`, label uuid `5dd178d8-06bf-4ab2-aed9-66575a76475b`.

`raw/capacity-before.txt` at launch: `running_rust_builders=0`, `host_cargo_rustc_clippy_rustup=0`. One own builder then ran.

Host command: `bash raw/probes/launch.sh`. Tool record duration 52.48s, exit 0. `docker-run.exit` is 0. HostConfig: `CapDrop=["ALL"]`, `Privileged=false`, `PortBindings={}`, `NetworkMode=oo-a100-5dd178d8-net`, tmpfs `/tmp` `rw,nosuid,nodev,noexec,size=1g`, `no-new-privileges`, memory 6442450944, NanoCpus 4000000000, PidsLimit 2048. `docker-state.txt`: `Status=exited ExitCode=0 OOMKilled=false`.

Inside, `raw/toolchain.txt`: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, `cargo 1.94.0 (85eff7c80 2026-01-15)`, `protoc` absent. `/proc/1/status` and `/proc/self/status`: `CapEff` and `CapPrm` `0000000000000000`, `NoNewPrivs` 1. Debian clang 19.1.7 `(3+b1)`, target `aarch64-unknown-linux-gnu`, from the nine mounted debs. `ldd.exit` 0. All nine `debs-compare.txt` lines are `match`.

Inside commands and exits, from `raw/phase.log` (2026-10-02T12:15:05Z through 12:15:55Z):

| Step | Exit |
| --- | --- |
| `cargo fetch --locked --manifest-path /src/Cargo.toml` | 0 |
| `cargo fetch --manifest-path /opt/probe-build/Cargo.toml` | 0 |
| `cargo build --offline --manifest-path /opt/probe-build/Cargo.toml` | 0 |
| `timeout -k 20 240 /target/debug/p5-identity-probe --probe` | 0 |
| inside | 0 |

`cargo test`, Clippy, and `--features test-hooks` were not run. The debug image is 33535112 bytes, under `MAX_WORKER_BYTES` (67108864). The release retry did not run. `probe.err` is empty.

## Harness failures kept

Attempt 1, `raw/attempt1/`: apt-get update and download under `CapDrop=ALL` exited 100. `ldd` exited 1. `inside.exit` 1. `docker-run.exit` 1. The probe never started. Cleanup of that attempt's container, network, and both volumes exited 0.

Attempt 2, `raw/attempt2/`: debs matched, `ldd` 0, fetch 0, fetch-probe 0. `oo-p05-pg17-parser` compiled. The probe crate failed: `KIND_RESPONSE` is not in the crate root, and `KIND_READY` was missing at the call. `build.exit` 101. `probe.exit` 127. `inside.exit` 1. `docker-run.exit` 1. Cleanup exited 0. The `apt-*` files in that snapshot are attempt 1 leftovers the second script did not rewrite. The attempt 2 phase starts at `2026-10-02T12:06:08Z`. Attempt 3 defines local frame kinds 1 and 3 and is the oracle below.

## Oracle

`raw/probe.out` SHA-256 `c8f6a6ce3b45bef1aaeb744fce599da1abf5ce94a96150c4219b432e036e6024`. Nineteen cases, all `ok` true. Summary: `fails` 0, `foreign_survived_all_cases` true, `sent_to_database` false, then the harness SIGKILL of its own sleep, `own_sleep_starttime_gone_after_own_kill` true.

Caller pin recorded on the image line: `10620a272a86bea1301b3ce899528a49e8e1ace4ed791bd956e2e9dadfec6a6f`. That is the SHA-256 of `/target/debug/p5-identity-probe`. It is a caller hash.

| Case | Stage | Code | Allow | SQL | Reaped | Ready/parse/cleanup/wall ms | Returned pid | Matched |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| pin-mismatch | pin | UNTRUSTED_WORKER | false | empty | false | 0/0/0/1536 | 0 | false |
| control | accept | ALLOW_SYNTAX | true | SELECT 1 | true | 1/0/1/1414 | 1854 | true |
| ready-nonce | ready | INVALID_OUTPUT | false | empty | true | 1/0/1/1400 | 0 | false |
| ready-pid | ready | INVALID_OUTPUT | false | empty | true | 1/0/1/1389 | 0 | false |
| ready-start | ready | INVALID_OUTPUT | false | empty | true | 1/0/1/1401 | 0 | false |
| ready-omit-nonce | ready | INVALID_OUTPUT | false | empty | true | 1/0/1/1487 | 0 | false |
| ready-omit-pid | ready | INVALID_OUTPUT | false | empty | true | 1/0/1/1394 | 0 | false |
| ready-omit-start | ready | INVALID_OUTPUT | false | empty | true | 1/0/1/1418 | 0 | false |
| ready-unknown | ready | INVALID_OUTPUT | false | empty | true | 1/0/1/1386 | 0 | false |
| ready-dup-nonce-last-wrong | ready | INVALID_OUTPUT | false | empty | true | 1/0/1/1397 | 0 | false |
| ready-dup-nonce-last-correct | ready | INVALID_OUTPUT | false | empty | true | 1/0/1/1485 | 0 | false |
| decision-nonce | decision | INVALID_OUTPUT | false | empty | true | 1/0/1/1421 | 1874 | true |
| decision-pid | decision | INVALID_OUTPUT | false | empty | true | 1/0/1/1411 | 1876 | true |
| decision-omit-nonce | decision | INVALID_OUTPUT | false | empty | true | 1/0/1/1394 | 1878 | true |
| decision-dup-nonce-last-wrong | decision | INVALID_OUTPUT | false | empty | true | 1/0/1/1395 | 1880 | true |
| decision-start-unknown | decision | INVALID_OUTPUT | false | empty | true | 2/0/1/1428 | 1882 | true |
| decision-matched-false | accept | ALLOW_SYNTAX | true | SELECT 1 | true | 1/0/1/1487 | 1884 | true |
| decision-oid-true | decision | INVALID_OUTPUT | false | empty | true | 1/0/1/1397 | 1886 | true |
| decision-matched-true-bad-nonce | decision | INVALID_OUTPUT | false | empty | true | 1/0/1/1429 | 1888 | true |

`foreign_alive` is true on every row for pid 1852 starttime `548271`. `proof_alive` is 0 on every row. `containment` is `not-applicable` on the pin case and `proven` on every spawned case. `workdir_removed` is true on every spawned case. `worker_identity` is `none` before spawn and `pinned` after a sealed image is launched.

### Control

Ready body nonce, pid 1854, and starttime `548566` are the live worker values. The worker saw admit kind 2, length 420, and sent the decision. The parent returned `ALLOW_SYNTAX`, `SELECT 1`, `detail` `a100-control`, `node_count` 7, `parser_version` 170007, `oid_acl_rls_proven` false, nonce equal to the session nonce, `starttime_matched` true, `reaped` true, cleanup proof `FINAL 0`.

### Ready identity

`screen_ready` runs before `write_request`. These nine cases never saw `KIND_ADMIT` (`saw_admit` false, `read_eof` true, `decision_sent` false). `parse_wait_ms` is 0, returned pid is 0, `starttime_matched` is false, code is `INVALID_OUTPUT`, SQL is empty, allow is false.

One field moved in each single-field case. `ready-nonce` changed only the last nonce nibble (`…c55d7c` to `…c55d7a`) and kept the live pid and starttime. `ready-pid` sent 99999 with the live nonce and starttime. `ready-start` sent starttime `1` with the live nonce and pid. Omit cases drop one required `ReadyBody` field. `ready-unknown` adds `injected`. Both duplicate nonce orders, last key wrong and last key correct, failed at this same gate with an empty returned decision. The decoder did not accept either key. This run does not show first-key or last-key wins.

Ready-reject proofs name the worker and end in `FINAL 0`. Example, `ready-nonce`: `PID 1856 548706 1855 1856 1` then `FINAL 0`. `proof_alive` 0 means that starttime was gone on the later `/proc` read. `reaped` true is tied to `containment=proven`, `workdir_removed`, `FINAL 0`, and `proof_alive` 0.

### Decision body

A valid ready frame is admitted. The worker reads kind 2 and writes `KIND_RESPONSE`. The fixed `Decision` struct has `nonce` and `pid`. It has no `starttime` field. `accept_worker_decision` blanks parent-owned flags, then `enforce_parent` checks the body. After that, `drive` writes `starttime_matched` from the ready screen and, when the ready pid is nonzero, overwrites `decision.pid` with that pid.

`decision-nonce` sent a flipped nonce and the live pid. The returned nonce is the flipped value (`…5fda`; live was `…5fdc`). Code `INVALID_OUTPUT`, allow false, SQL empty, `detail` still `a100-control`, `node_count` still 7, `parser_version` still 170007. That is `enforce_parent`'s `reject`: it clears allow, the oid flag, and SQL, and it leaves detail, node count, and nonce. The native comparison frame is detail empty and node count 3, so the parent did not repair the reply with `admit_native`. The decision nonce is checked. The ready channel also binds the session nonce. A wrong decision nonce is still a bounded `INVALID_OUTPUT`.

`decision-pid` sent pid 99999 and the live nonce. The returned pid is the ready-bound worker pid 1876. Code `INVALID_OUTPUT`, SQL empty, detail and node count preserved. The spoof is in the worker note (`sent_decision_pid` 99999), and the returned pid is the process the ready frame bound.

`decision-omit-nonce` and `decision-dup-nonce-last-wrong` and `decision-start-unknown` come back with detail empty, node count 0, nonce empty, and `parser_version` 0. That is `Decision::deny` from a serde failure, which is the `from_slice` error path in `accept_worker_decision`. The duplicate body contained the live nonce and a flipped second nonce. The unknown-field body added `starttime`. None of these three kept `a100-control`. All three had already passed the ready gate (`saw_admit` true, `decision_sent` true, `starttime_matched` true).

`decision-matched-false` sent `starttime_matched` false with the live nonce and pid. The parent returned the control allow, with `starttime_matched` true. The worker flag is blanked before enforcement. The returned bit is the ready-screen result.

`decision-matched-true-bad-nonce` sent `starttime_matched` true and a flipped nonce. The result is the enforce reject path: detail and node count kept, SQL cleared, returned `starttime_matched` true. The worker flag did not carry the bad nonce.

`decision-oid-true` sent `oid_acl_rls_proven` true with a live nonce and pid. `enforce_parent` rejects that claim first. The return has the flag false, SQL empty, detail and node count kept. The parent refuses the worker's proof field. OID, ACL, and RLS authorization remain unproved.

### Pin, clocks, foreign process

`pin-mismatch` flips the first byte of the caller hash. Result: `UNTRUSTED_WORKER`, identity `none`, pid 0, `reaped` false, containment `not-applicable`, empty proof, ready/parse/cleanup all 0. The image is refused before spawn.

Spawned ready waits are 1 ms, or 2 ms on `decision-start-unknown`. Parse wait is 0. Cleanup wait is 1 ms. Wall is 1386 ms through 1536 ms. The probe bounds were 2000, 700, 700, and 20000. The wall includes the memfd copy of the 33535112-byte image, outside the ready ceiling. a091's pinned `SELECT 1` wall was 1441 ms with ready/parse/cleanup 1/1/1.

The sleep at pid 1852 starttime `548271` was alive after every case, including both pid spoofs. The harness then SIGKILL'd that child and saw the starttime gone. The parent proof lines do not name 1852.

Control and decision proofs are the single line `FINAL 0`. `proof_alive` is 0 because those proofs have no `PID` line. `reaped` true there is the parent's result: containment `proven`, workdir removed, `FINAL 0`, no `SCAN_ERROR` or `UNAVAILABLE`. The parent's `proof_holds` also requires the pre-kill descendant snapshot starttimes to be gone before it sets `reaped`. This probe did not record a second `/proc` read of those worker pids. No row has `reaped` true with `proof_alive` greater than 0. The direct starttime-gone observation is the ready-reject proofs, which name the pid.

## Proved

- Production `admit_in_worker` through `--supervise`, default features, caller pin equal to the image hash, on the fixed archive.
- Live ready nonce, pid, and starttime plus the distinctive `ALLOW_SYNTAX` / `SELECT 1` frame is accepted, and the native comparison frame stays distinct.
- A single ready nonce, pid, or starttime change is `INVALID_OUTPUT` before the admit frame, with SQL and allow clear.
- Omitted ready nonce, pid, or starttime, and an unknown ready field, fail at that same gate.
- Duplicate ready nonce keys fail at that gate for both orders.
- A parsed decision with a wrong nonce or a wrong pid is `INVALID_OUTPUT` after admit. SQL and allow are cleared. Detail `a100-control` and node count 7 remain, so the native parser did not replace the body. The returned pid on a pid spoof is the ready-bound pid.
- An omitted decision nonce, a duplicate decision nonce, and a decision `starttime` key fail serde and come back as `Decision::deny` shape, still after a matched ready, with SQL clear.
- A worker `starttime_matched: false` does not clear the parent's ready match on a valid identity.
- A worker `starttime_matched: true` does not carry a bad nonce.
- A worker `oid_acl_rls_proven: true` is cleared and the allow is rejected.
- A wrong caller hash is `UNTRUSTED_WORKER` before spawn, with `reaped` false.
- Ready-reject proof lines show the named worker starttime gone, with `reaped` true only alongside `containment=proven` and `FINAL 0`.
- Pid 99999 was absent and was not the foreign sleep. Sleep pid 1852 starttime `548271` survived every case.

## Unproved

Full P5, R4, R5, R9, Gateway, OID/ACL/RLS authorization, native 17.7 versus server 17.10, network, source I/O, pid namespace, runsc, supply, amd64, and release stay unsigned. Containment here is one arm64 bridge-network run of this image. ECHILD/EINTR injection, cgroup limits, and `RLIMIT_NPROC` were not probed. A decision `starttime` field does not exist on `Decision`; only an unknown key was tested. Duplicate keys were rejected, so first-key and last-key acceptance were not observed. Worker pids whose proof text is only `FINAL 0` were not re-read from `/proc` by this harness. Corpus, Clippy, and `test-hooks` were not repeated. The caller hash is not an external signature.

## Cleanup

`docker rm oo-a100-msrv-5dd178d8`, `docker network rm oo-a100-5dd178d8-net`, and `docker volume rm` of both a100 volumes exited 0. No prune. `raw/inventory-after.txt` has no `oo-a100` name. A later filter on that container, those volumes, and that network was empty. Other containers were left in place.

Hash list: `raw/hash-matrix.txt`, SHA-256 `24aa25814a993abf1fc02c6e0b648b3c5b2b5b102dd132b0480c8d59ad664162`.
