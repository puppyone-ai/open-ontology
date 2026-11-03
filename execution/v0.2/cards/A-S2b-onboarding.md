## Card A-S2b — lane A: MySQL doctor, rest of the TUI, lite compose and oo up/down/status

TODO: S1-A-02 (MySQL part), S2-A-01, S2-A-02. Rebase on v0.2/product-loops first. Integrated now: MySQL,
PostgreSQL and Oracle SourceEngine adapters; your Oracle doctor commit (0c9160cdb). Your TUI and lite
bootstrap commits stay on your branch until the S1 exit. Lane M is building `oo-load` (engine-neutral load
into the Agent DB); until it is integrated, the draft step may call the existing PostgreSQL path, and must
switch to `oo-load` when it lands (the coordinator will tell you via a follow-up card if needed).
Work rule: smallest local workaround and keep going; commit as you go; `blocked` only if no workaround exists.
1. S1-A-02: register MySQL in the doctor registry (5.7.44/8.0.44/8.4.7/9.5.0 via oo-test fixtures; 5.7 amd64
   emulated), byte estimates through estimate_bytes. Record results.
2. S2-A-01: finish the TUI — execution/progress panel (per-stage progress, throughput, failures/retries, quit
   and resume via `oo status` / `oo build --resume`) and the completion screen (Ontology export path, Pack path,
   Gateway URL, first token via enroll, agent prompt via share-prompt — wire to what exists, label what is
   pending). Keep the PTY byte-equivalence and secret-scan tests green; extend them to the new screens.
3. S2-A-02: deploy/lite compose with pinned digests (Agent DB postgres, control DB, worker, Builder, Gateway,
   model-egress, scheduler), Dockerfiles under deploy/lite/ for linux/amd64 and linux/arm64 built from this repo,
   `oo up/down/status` wired to it (status: last success, last failure, next due, nearest credential expiry from
   /v1/health), first `oo up` uses your credential bootstrap. Exit: `oo up` on this Mac brings the stack up and
   `oo status` reports healthy; then one command from the PostgreSQL fixture to an unreviewed draft through
   model-egress (`gpt-6.1-sol`, key via secret_ref file ~/.config/open-ontology/secrets/model-key-9r-sol, bounded
   live usage). Then Oracle 23ai and MySQL 8.4 drafts if time remains.
Disk is limited (~25 GB free): build images one at a time, remove dangling images you built, label all.
