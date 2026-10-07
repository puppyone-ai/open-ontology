# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a346`. Verdict `APPROVE_LIMITED` for one trusted-operator actual natural-language chain. The question was "How many stored rows are in depot.counter?" The request model was `grok-4.7`. The completion label is `grok-4.7-build`. The SQL `SELECT count(*) FROM depot.counter` was approved after the proposal and executed on private Postgres 17.10. The receipt is `linux/execute/execute.stdout`: `query_id` `q_8bd1eadb5903e12d`, row `int` `3`, session `sess-a346`, citation `b78ee58389eca7216fc7f6798404c2fc71fe5a528b83f3e11de7e410f08b5a20`.

Do not treat this as parent acceptance, goal acceptance, Builder 100k, I01, I02, public authentication, or harness acceptance. SP11-W03 and RT-D05 stay open. `authenticated` and `authenticated_audit` stay false.

The raw request and completion are `linux/propose/request-body.json` and `linux/propose/completion.json`. a334 did not keep those bodies for its headcount receipt. The relay 28-test proof is reused from a334 proof-3 because the 21-file listing matches commit `8c825d89`. Dialogue and `query-pg` were built from archive `63566b60`.

Failed attempts kept here: `linux/smoke/attempt-1` and `host/attempt-listen-1`. The execute script was removed by its own marker scan and restored; see `linux/execute/scan-note.txt`.

Owned cleanup stops the relay and observer from the evidence pid files, matching the process basename. The private directory has no pid files, and macOS `ps` comm is a full path. It then removes the a346 Postgres container, volumes, network, and `/var/tmp/oo-a346-eef831aa-dba4-4af2-a78b-541bc9e0a346`. It leaves the proxy on 8317, the upstream token file, and foreign containers. It does not prune and it does not open an auth store. Historical `auth_files_restored=2` remains only in the a334 packet.

`SHA256SUMS` is the source inventory for this directory. The tree has no token file, password file, or private key. A byte search of the upstream token file matches `tools/sha256.rs` and `oracle/observe.rs` as a substring. Those reviewed sources were kept, and the match was not printed.
