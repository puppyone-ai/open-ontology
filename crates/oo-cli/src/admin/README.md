# Admin commands

`oo init --config stack.yaml` validates first and atomically normalizes the
same file as JSON, mode 0600. Failed validation leaves the original untouched.
This non-interactive path never resolves secret references.

`oo doctor [--config stack.yaml] [--json]` uses the generic `SourceEngine`
probe and chunk-plan interface. Its registry is keyed by `EngineFamily`;
PostgreSQL is wired, MySQL and Oracle explicitly report
`engine not available in this build`. Compatible families are not aliased.
`auto` selects a candidate on standard ports only; the adapter verifies the
actual version. Nonstandard ports need an explicit engine.

PostgreSQL requires `tls_ca: {"secret_ref":"source-ca"}` and resolves that
trust anchor and the password through the owner-private secret store.
Hostname verification stays enabled. Explicit `schema.table` include entries
are supported; exact table, schema, `schema.*` and `*` exclusions take
precedence. Wildcard include discovery is not exposed by the adapter, so it
returns a blocked scope diagnostic alongside the basic probe, not an empty
successful plan. SSH remains unavailable.

JSON contains a source-indexed ProbeReport set, capability states, DBA
requirements, chunk counts/strategies and row estimates. Byte estimates are
null with an explicit unavailable basis: the engine API does not expose them.
Table planning failures preserve the successful probe and DBA advice.
Missing snapshot prerequisites block preflight; missing C1 prerequisites are
advice for auto/C5 and blocking when C1 is explicitly required. DBA SQL is
printed only, never executed.

The model probe runs through embedded model-egress, with validated JSON on
stdin, no credentials in argv and no persistent meter. Its window/output
limits retain their metadata/fallback basis; overhead is minimal-prompt total
usage, not an isolated measurement of injected instructions.

Blocked reports exit 1 while retaining the complete report on stdout.
Config/argument failures retain the existing sanitized CLI error envelope.
Consumer `status <query_id>` remains unchanged.

## Validation

```sh
cargo check --workspace --locked
cargo test -p oo-cli -p oo-stack --locked
python3 -m unittest discover -s tools/model-egress/tests -v
python3 crates/oo-cli/src/admin/test_doctor.py --run-id a-doctor-new-run
```

The last command requires the locally cached PostgreSQL images named in the
script and a built `target/debug/oo`. It uses the lane-H synthetic generator
at reduced scale, one lane-A labelled container at a time, verified TLS, a
separate SELECT-only principal, and a replay model on loopback. Runtime
passwords and TLS keys stay in ignored private evidence directories. It
removes only its own container after checking both labels. It does not
validate a configured live model or produce a Self-build draft.

## Remaining card exits

This is a partial S1-A-02 implementation, not full onboarding acceptance.
Generic discovery, byte estimates, no-key exported snapshot ownership and
MySQL/Oracle integration need follow-up. No ratatui/crossterm dependencies
have been added or admitted. S2-A-01 (seven-step wizard/PTY equivalence) and
S2-A-02 (lite images, lifecycle, mTLS bootstrap and real-model draft) are not
implemented. Their existing commands continue to fail explicitly; there is
no success placeholder or unaudited compose bundle.

`coordinator-source-engine.patch` proposes backward-compatible discovery and
byte-estimate methods on the coordinator-owned trait. It is not applied;
`git apply --check` passes. The defaults deliberately refuse discovery and
return unknown bytes. PostgreSQL overrides, scope selection wiring and their
tests are still required; applying this contract alone completes no exit.
