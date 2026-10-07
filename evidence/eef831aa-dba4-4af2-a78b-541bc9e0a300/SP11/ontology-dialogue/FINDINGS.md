# Findings

Author check of the proof-8 oracle and the owned source. This is not an independent second review and it does not close the parent ledger.

## F1. Binary read cap

`query-pg` at `/target/debug/query-pg` in the private target volume was a regular file of 160236912 bytes. `BINARY_MAX` was `128 * 1024 * 1024`. `read_regular` returned oversize, and `execute` mapped that to `binary` before comparing the hash and before spawn. Proof-7 stderr is the panic `execute binary` at the live `execute` call. The contract's exit 5 is the hash mismatch. The size bound is local. It is now `256 * 1024 * 1024` in `src/read.rs`. Proof-8 spawned the same binary.

## F2. Counts

The north success path is the visible projection `SELECT headcount FROM depot.counter ORDER BY headcount`. Returned rows are 3. `SELECT count(*) FROM depot.counter` also completed on this fixture: exit 0, receipt `clean`, cell `3`. Both are in `linux/proof-8/live/oracle.txt`.

## F3. Citations

`linux/proof-8/live/north-execution.json` embeds the answer-citations object. `trust` is `local_operator_declared`. `authenticated_audit` is false. `entailment` and `warning_comprehension` are `not_judged`. `query_id` `q_3782300da36af286` is the admitted id in that object. The execution field copies it. The fingerprint is 16 lowercase hex digits.

## F4. Onboarding

`pg-onboarding` with `--help` exits 2 and prints `pg-onboarding: usage usage`. The proof does not claim a shop onboarding run. `install-column-acl` north and south, and `provision-mapping-reader`, exited 0. Their stdout is under `linux/proof-8/live/`.

## F5. Libclang log line

`linux/proof-8/console.log` contains `ls: cannot access '/cargo-home/a300-clang-root/usr/lib/aarch64-linux-gnu/libclang.so*'`. The following ldd of `libclang-19.so.19` exited 0 and resolved `libLLVM.so.19.1` from that private root. `libclang-load.exit` is 0. `query-pg` then built.

## F6. Bounds that stayed

Propose uses one HTTPS POST, redirect policy none, retry never, no proxy, HTTP/1 only, and the fixture CA. A declaration is not treated as a certificate. Model SQL is not auto-executed. Stale Pack and SQL tamper return `approval` before the child. Guard exit 6 leaves no audit file and no `query_id`. Credentials are not written to the model body or the oracle.

`#![forbid(unsafe_code)]` is set on the library and the binary. No root manifest or root lock edit is in this change.

## F7. Left to parent work

Hosted Gateway authentication, untrusted parser admission, `runsc`, and the NL harness are outside this command. The local approval is not authenticated authentication. A local-LLM path owned by another session is not this transport.
