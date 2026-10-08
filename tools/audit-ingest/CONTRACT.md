# Audit ingest

Contract `oo.audit.ingest/v1`. Session `eef831aa-dba4-4af2-a78b-541bc9e0a371`.

This is the collector prerequisite for the gateway audit WAL. It is a private rustls mTLS receiver and a PostgreSQL 17.10 schema named `oo_audit_ingest`. It does not accept SP02-W06. The feedback receiver is not implemented. It does not accept SP06-W06, the gateway parent, or a release.

## Identity

The workload identity is the URI subject alternative name on the client certificate presented on the TLS connection. `schema/registry.md` is the line format. One URI maps to one project. The JSON `project` field must equal that project. `X-Project` and every other header are ignored. A certificate URI that is not in the registry is refused. A JSON project that does not match the registry project is refused.

## HTTP

`POST /v1/audit-events` with `Content-Type: application/json`. The body matches `schema/event.v1.json`. The response matches `schema/ack.v1.json`. Other methods and paths are refused. There is no public read API and no admin API.

The receiver recomputes SHA-256 over the decoded payload bytes and requires it to equal `payload_hash`. The payload is at most 16384 bytes.

## PostgreSQL

The schema is `migrations/oo_audit_ingest_v1.sql`. The receiver connects as `audit_ingest` only. That role may `INSERT` and `SELECT` `event` and `receipt` for dedup. It may not `UPDATE`, `DELETE`, or `TRUNCATE` those tables. It has no grant on other schemas, on `pg_authid`, or on config and secret tables. This crate does not create those tables.

One transaction inserts the event and the receipt, or reads the existing receipt. The key is `(spool_id, request_id, event_seq)`. The same payload hash returns the original `receipt_id`. A different payload hash conflicts, commits nothing new, and the HTTP status is 409. The gateway query path is not given this role or this password.

## Files

The process takes `--config <path>` and no other arguments. The config points at certificate, key, registry, and password files. Key and password files must be mode `0600` or `0400`. Their bytes are not written to logs, argv, the environment, or git.

## Out of scope

Feedback intake, retention policy beyond the rows this receiver inserts, and any claim that the parent collector work item is done.
