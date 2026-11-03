# Lite integration status (lane A, card A-S2)

Not a runnable deployment yet. Do not treat the credential tests or configuration
wizard as acceptance of `oo up`, a source-to-draft run, or publication.

## Delivered foundation

`oo_stack::credentials::bootstrap(secret_dir)` creates a private internal CA,
distinct Ed25519 mTLS identities for Agent DB, control DB, worker, Builder,
Gateway, model-egress, scheduler and admin, and a random administrator password.
Certificates last 730 days. The directory is 0700; files are 0600. Existing
credentials are never overwritten or silently rotated. A partial bootstrap
fails closed and requires operator recovery.

`credentials::status(secret_dir)` verifies certificate/key correspondence and
CA trust, reports the earliest CA/leaf expiry, and flags expiry within 14 days
or already expired. It does not measure Gateway token expiry. It does not
claim mTLS is enforced by services: that requires the compose integration.

Requires OpenSSL with Ed25519 support and Unix secret-store semantics. No
host secret directory was used in acceptance tests; generated keys lived in
owned test directories and successful tests remove them.

## Blocking integration prerequisite

At this card's base `9a378b86c`, the coordinator-owned
`execution/v0.2/cards/M-S1b-engine-load.md` explicitly records incomplete
engine-neutral admission and assigns the `oo-load --stack ... --target ...`
entry point to lane M.

The available `crates/oo-pipeline/src/engine_chunk.rs` validates and decodes
receipts, but explicitly does not grant target-write admission. The existing
`crates/oo-pipeline/src/native_c5_parts.rs` still rejects a `source_engine`
other than MySQL. Neither worker nor Builder directly consumes `oo.stack/v1`.
Lane A must not impersonate MySQL, bypass admission, add ad-hoc target SQL, or
edit the coordinator's pipeline/worker/migrations to claim this exit.

Consequently no placeholder compose services, fabricated image digests or
idle containers are presented as a working stack. `oo up/down/status` remain
explicitly unimplemented. The next integration needs:

1. Lane M's admitted, resumable source-to-Agent-DB entry point and target config.
2. Compose wiring and repository Dockerfiles for both architectures, followed
   by actual image builds and immutable image locks.
3. Runtime integration of bootstrap, source loading, model-egress/Builder,
   scheduler and Gateway; status must read actual success/failure/due times
   and token expiry rather than invent values.
4. PostgreSQL and Oracle 23ai fixtures to an **unreviewed** draft using the
   authorized model endpoint/key with bounded usage. No live call was made
   by this card. No review, publish or token issuance may be inferred.

## Evidence

- `a-s2-doctor-healthy`: reduced lane-H Oracle fixtures, 23.5/21/11; missing
  FLASHBACK grants block with DBA advice, separate fixture-admin grants allow
  healthy preflight. Model responses are synthetic replay.
- `a-s2-tui-pty-final`: real PTY configuration entry, byte equality with an
  independent non-interactive normalization, terminal/config secret scan,
  and secret modes. MySQL is deliberately unavailable in this case.
- `a-s2-tui-v2`: exact dependency graph audit/deny and dependency-record checks.
- `a-s2-lite`: workspace check, CLI/stack tests and source evidence of the
  coordinator-owned admission prerequisite. See `evidence-summary.json`.

These are local author checks, not full AT-ONB-02 or S2-A-02 acceptance.
