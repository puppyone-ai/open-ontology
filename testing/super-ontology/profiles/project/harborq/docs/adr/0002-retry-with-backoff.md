# 2. Retry failed jobs with exponential backoff

Status: Accepted

## Context

Transient failures are common when workers call external services. A fixed delay either wastes time or overloads the service.

## Decision

Workers retry a failed job with exponential backoff and jitter. The first delay is 2 seconds and each delay doubles.

## Consequences

- A job is retried at most 5 times and then moved to the dead letter table.
- The delay is computed by next_delay in core/src/retry.rs.
- Workers must not retry a job that failed with a validation error.
