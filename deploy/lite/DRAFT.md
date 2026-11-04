# Bounded metadata-to-draft bridge

## Source-to-Agent-DB draft

```sh
oo build --config stack.yaml --agent-target target.json --json
```

The optional private (0600) target file uses the existing `oo-load` JSON
contract: `host`, `port`, `database`, `user`, `password.secret_ref`,
`tls_ca.secret_ref`, and `run_id`. Install `oo-load` beside the CLI. The target
must be a dedicated, authorized Agent DB; verified TLS is required. Target
secrets live in the same owner-private store as source references.

For PostgreSQL, MySQL and Oracle, this command performs read-only source
preflight and snapshot loading, checks one receipt per selected table, and
passes only the published `oo_load_data` views to Builder. It reports
`agent_db_load: local_table_views`, `consistency: B`, `source_time: unmeasured`.
These local table views are not Gateway publication or owner review. Target
configuration participates in the resume fingerprint; a completed loader
`run_id` replays durable receipts rather than creating another generation.
Use the same target flag when resuming.

The fixture runner `test_loaded_draft.py --engine postgres|mysql|oracle
--run-id <new-id> --image <runtime-digest> --live` provisions synthetic source
and target databases, then executes that one command. At most two databases
run at once; the runner deletes only its own resources. Model endpoint/key are
read through private secret files. Each successful one-table run needs one
generation request plus preflight probes; there is no generation retry.

## Legacy metadata-only mode

`oo build --config stack.yaml` runs preflight, catalog, profile, comments, seal,
cards and generation. Install `pg-builder` and `oo-pipeline` beside `oo`.
`oo status` reads `.oo/run.json`; `oo build --resume` retries failed/interrupted
stages and verifies hashes before reusing completed artifacts. Configuration
changes or modified artifacts refuse resume. Each attempt remains counted.
The checkpoint directory is private and builds are serialized by a file lock.

TTY execution shows stage completion, operation throughput, failure codes and
retry counts. q/Esc/Ctrl-C terminates the owned process group and retains the
checkpoint. Completion distinguishes the unreviewed `.oo/draft.json` from all
pending publication outputs. Non-TTY execution emits the final JSON report.
Raw source values, model keys and long-lived tokens are never progress text.

Without `--agent-target`, this bridge reads PostgreSQL metadata/statistics and
does not load the Agent DB. Both modes support one
PostgreSQL source, explicit table scope, at most 16 tables and at most two
8-column generation batches. Oversize input fails, never silently truncates.
Generation allows at most 8192 output tokens and 60000 total tokens per call;
preflight additionally makes two minimal probe completions. No automatic retry
of a paid generation request occurs. An explicit resume can incur another call.
The existing Builder validates model output; structural drafts do not pass.
Generation permission approves sending metadata only, never approves output.

Builder requires its existing loopback port 8317. An occupied host listener is
never reused or stopped: use the repository container's isolated network.
No export, review, Pack, publication, token issuance or Agent DB acceptance is
implied. These completion fields say pending until the actual integrations exist.

Author evidence: `a-s2b-pty-5` (real PTY and synthetic model replay),
`a-s2b-live-draft-2` (owned PostgreSQL 17 fixture, one real gpt-6.1-sol generation
through egress: 960 prompt / 532 completion tokens, plus two preflight probes).
Both are narrower than S2-A-02 acceptance. Failed earlier runs remain preserved.
