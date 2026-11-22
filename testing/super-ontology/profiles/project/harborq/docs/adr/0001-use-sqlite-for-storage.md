# 1. Use SQLite for storage

Status: Accepted

## Context

Harbor Queue targets small services that cannot run a separate database. Operators want a single file they can back up.

## Decision

We will use SQLite as the only storage engine, opened in WAL mode.

## Consequences

- Deployment is a single file next to the binary.
- Writers are serialised, so bulk inserts go through the batch API in core/src/queue.rs.
