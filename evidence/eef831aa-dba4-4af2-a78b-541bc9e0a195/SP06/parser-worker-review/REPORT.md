# a195 parser worker review

Verdict: **APPROVE LIMITED**

Reviewed source: `c22cf08aa5973ae2e2d95e0d2b72f98487f8555a` (`gateway: isolate one SQL guard call in a one-shot child`). Tree `adf69fac5724c2635566fe75f2b9e1391fdd5fcd`. Author evidence `a63f2de3fd411532e11f5863c0f595724ad6f149`. This review did not edit product source, `Cargo.toml`, `Cargo.lock`, or the ledger.

The one-shot supervisor matches `contracts/gateway/parser-worker-v1.md` for a direct child: one request, `guard_select` in the child, a 500 ms clock that covers the pipes, `SIGKILL` of that pid, and `wait`. `RESTRICTION_CAPABILITY` is `Unsupported`. `UNTRUSTED_ADMISSION` is `Blocked`. A grandchild that inherits the child's stdout keeps the parent's read blocked after the direct child is dead. That limitation is in the contract and was measured. It is not a kernel sandbox and it is not a public launcher.

SP06-R8 stays open. Public untrusted admission stays closed. This review does not accept authentication, the public gateway, or a release.

## What ran

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. No pull. No PostgreSQL. Private network `oo-a195-eef831aa-net`, volumes `oo-a195-eef831aa-cargo` and `oo-a195-eef831aa-target`. Each container: `--cap-drop ALL`, no-new-privileges, memory 8g, cpus 4, pids-limit 4096, `HOME=/tmp`, tmpfs `/tmp` `noexec`. Host home was not the container home. No docker socket. No host network.

The immutable archive is a `git archive` of `c22cf08` at `/tmp/oo-a195-eef831aa-dba4-4af2-a78b-541bc9e0a195/archive` (27136 files, matching `git ls-tree`). The review overlay was copied only into a private proof tree and mounted read-only at `/src`. `parser_worker.rs` blob `aabf78ad16f621ea5fd82bc4fa94548c28895767` on both the archive and the proof tree.

`Cargo.lock` stayed `1e9b9377cb2bdf951ab22f7f0a4b68649cb70e563f6ef13adad5852e1c72c657`. `cargo update` was not run.

## Pre-overlay smoke

`linux/smoke/attempt-1/` is the failed loader attempt. `ld-linux` was invoked on an empty path because Debian ships `libclang-19.so.19`, and the first script looked up `libclang.so` in the multiarch directory. Extracted packages and the `stdbool.h` check had already succeeded.

The rerun pointed `LIBCLANG_PATH` at `libclang.so` under the private LLVM directory and loaded `libclang-19.so.19`. `libLLVM.so.19.1` resolved from the private root (`linux/smoke/libclang-ldd.txt`). No `not found`.

| Phase | Exit |
|---|---|
| sha256 tool, select-debs, owned hash, libclang, includes, load | 0 |
| fetch, `cargo test -p oo-gateway --lib --test parser_worker` | 0 |
| `cargo clippy -p oo-gateway --all-targets -- -D warnings` | 0 |
| lock hash, unsafe scan, smoke, docker | 0 |

Library tests: 11 passed. `tests/parser_worker.rs`: 27 passed in 1.52s. The `sql_guard` AST campaign was not run. Container `oo-a195-smoke` id `9b0ed39883acbfdbec574601aa72930afe1a3f87287283b5a8ec172644783323`, exit 0, not OOM, not privileged, then removed.

## Process proof

`linux/proof/attempt-1/` failed in the harness before a supervisor run (`str::get` on a byte index, and panics that reused a moved `Result`). `linux/proof/attempt-2/` ran the 11 review tests to success in 8.07s and then failed because a new container does not keep the smoke container's rustup clippy component.

The recorded proof installed clippy (`linux/proof/clippy-component.log` downloaded the component), then:

| Phase | Exit |
|---|---|
| overlay present, product hashes equal to the smoked archive | 0 |
| `cargo test -p oo-gateway --test parser_worker_a195_review -- --test-threads=1` | 0 |
| clippy component, `cargo clippy -p oo-gateway --all-targets -- -D warnings` | 0 |
| lock hash, proof, docker | 0 |

11 tests passed in 7.90s. Clippy finished with exit 0. Container `oo-a195-proof` id `a138f389de491a0556f7ffb006a2d9aa2d85f0080b385ab53717eb59a122cfdb`, exit 0, not OOM, not privileged, capabilities dropped `ALL`, then removed.

The oracle is `linux/proof/oracle.txt`. Digests inside it come from the `sha2` crate in the test. File manifests come from `tools/sha256.rs`.

## Measured behavior

Fresh catalog `depot` / `oo_r_clerk` / `policy-a195-fresh`, relation `depot.parcel` mapped to `_oo.pt_parcel`. The worker plan equaled in-process `guard_select` for:

- `SELECT/*a195*/ parcel_id, zone FROM depot.parcel WHERE parcel_id = 4` (operator OID 8802, wall 1 ms)
- `SELECT * FROM depot.parcel WHERE zone = 'east'` (one star, columns `parcel_id`, `zone`, operator OID 8804)
- `SELECT count(*) FROM depot.parcel WHERE parcel_id = 4` (function OID 8801 `aggregate_star`, operator OID 8802)

Identity mismatch on the canary SQL displayed and debugged without that SQL. Codec checks covered an optional tag of 2, a `u32::MAX` string length, a `u32::MAX` count, 129 relations against the 128 cap, closed refusal labels, trailing frame bytes, and an oversize header. `u32::MAX` returned `Limit` in under 50 ms.

Direct-child faults, all reaped, pid absent, start time gone:

| Case | Wall | Elapsed | Signal or exit |
|---|---|---|---|
| slow | 503 ms | 503 ms | signal 9 |
| crash | 1 ms | 1 ms | exit 99 |
| oversize | 2 ms | 2 ms | signal 9 |
| magic, kind, trailer | 0 ms | 0 ms | exit 0, reaped |
| cancel before spawn | 0 ms | — | no pid |
| cancel after spawn | 0 ms | 0 ms | signal 9 |
| blocked stdin, 1,513,710 byte request | 508 ms | 500 ms | signal 9 |
| hold stdout | 501 ms | 501 ms | signal 9 |
| partial header | 504 ms | 504 ms | signal 9 |
| late frame after 700 ms | 501 ms | 501 ms | signal 9, timeout |

The late reply was a timeout. The next production child was a different pid and returned a plan in 2 ms. An unrelated `/bin/sleep` kept its start time. A `/bin/sleep` grandchild kept its start time after the direct `SIGKILL`. The production binary exited 2 for `--slow` and for `OO_PARSER_WORKER_SLOW`.

## Limits of this approval

`linux/proof/oracle.txt` records `restriction=Unsupported` and `admission=Blocked`. The probe child shared net, mount, pid, and user namespaces with the parent, opened `/etc/passwd`, and bound `127.0.0.1` (port 42149). Its environment was empty and its process group id was its pid. The fd table was `0` pipe, `1` pipe, `2` `/dev/null`. A parent `File` on `/etc/hosts` did not appear (`hosts_fd=false`); Rust opens that file with `CLOEXEC`. `spawn_worker` does not close any other inherited descriptor. This run is evidence that the process is not isolated. It is not evidence of a sandbox.

`h_grandchild_stdout_is_not_a_sandbox` did not return within 3 s. The grandchild still held the stdout pipe. The test sent `SIGKILL` to that grandchild, and the join then returned `TimedOut` with signal 9 on the direct child and elapsed about 3.01 s. `finish` kills only the direct pid and then joins the read thread. A grandchild outside that pid is not reaped. The contract already says so.

`read_frame_from` accepts a request body and ignores bytes after it. A direct spawn of `oo-sql-parser-worker` with a trailing `0x7f` exited 0 and wrote 266 reply bytes. `decode_frame` on the same buffer returns `Trailing`. The supervisor path rejects a trailing reply byte. The child reader does not.

`query_executor.rs` still says a 500 ms worker is not implemented (`UntrustedAdmission::Blocked` at the enum, and the module comment above it). The constant remains `Blocked`. The file was not edited.

Magic, kind, and trailer faults had already exited 0 when `wait` ran, so the report shows exit 0 and an empty signal together with a reaped, absent pid. `Child::kill` does not rewrite a status the process already has.

## Artifacts

- `source/COMMIT`, `source/blobs/`
- `linux/smoke/` including `attempt-1/`
- `linux/proof/oracle.txt`, `test.log`, `clippy.log`, `hostconfig.txt`, `mounts.txt`, and `attempt-1/`, `attempt-2/`
- `overlay/parser_worker_a195_review.rs`
- `tools/sha256.rs`, `tools/select-debs.rs`
- `SHA256SUMS` from `tools/sha256.rs`
