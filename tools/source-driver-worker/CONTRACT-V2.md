# oo.source.driver-oracle/v2

This contract sits beside `oo.source.driver-worker/v1`. It does not change v1 frames, the v1 `k`/`v` schema, or v1 `kernel_network=pending`.

The supervisor binary is `oo-source-driver-p2`. It links Arrow IPC 60, JSON, and SHA-256. It does not link `oracledb`. The thin client lives only in `oracle-child`, a separate Cargo workspace.

## Launch

The caller passes one mode-0600 JSON file. The supervisor reads the image id from the Docker engine and refuses to create a container when that id is missing or differs. The child container is user `65534:65534`, capability drop `ALL`, read-only root, no-new-privileges, at most 512 MiB and 0.5 CPU, and one internal network. Environment keys are `HOME` and `TZ`. Mounts are the source password file read-only, `/audit`, and a `/tmp` tmpfs. A mount of `docker.sock` is refused.

The password is not a JSON field, not an argument, and not an environment value. `secret://{deployment}/{project}/source/{secret_id}` maps to a mode-0600 file. That mapping is not the secret-control HTTP service.

The child statement is an allowlist. Caller SQL is not accepted. The business statement is `oracle:batch-asof` against `OO04_FIX.T_BATCH` in `FREEPDB1` on port 1521. The SCN comes from `MAX(ora_rowscn)`.

## Batch

The child writes one Arrow IPC stream, one lineage frame, and one completion. The accepted Arrow types are Int16, Decimal128(16,6), Int8, Timestamp(Microsecond, None), Utf8, Utf8, and Binary, in that column order. Float64, Boolean, unconstrained NUMBER, TIMESTAMP(9), and time-zone values are not accepted. `admits_import` is false. The batch statement does not interrupt a call, so its completion frame records `cancel_api=absent`. That string is not an OCI break. The break is measured only by `oracle:oci-cancel`.

`kernel_network` on a v2 receipt is `internal`. That word describes the Docker network mode checked on the running container. It is not a v1 certificate and it does not promote a matrix grade.

## Cancel and deadline

`oracledb` 26.0.0-beta.4 has `set_call_timeout` and `ErrorKind::CallTimeoutExceeded`. It has no explicit cancel API. A call timeout is recorded as a deadline, not as cancel.

The deadline proof runs the nested `ALL_OBJECTS` select with a 3 second call timeout and requires the client error to be the call timeout. The server statement must not still be `ACTIVE` afterward. A DBA kill is not part of that proof.

The cancel proof uses the same select with a 90 second call timeout so the SDK timeout does not fire first. After `v$session` shows the statement `ACTIVE`, the child calls `Connection::request_cancel` from the overlay. The published crate has no such method. `Cargo.lock` still records the published crate checksum. The patched extract is a separate tree. `cancel_api=oci_marker_interrupt` is written only when that call returns `ErrorKind::Cancelled`. The measured proof is that command's stdout, stderr, and exit, plus the server statement no longer `ACTIVE`. A container kill is not the success proof. A string returned without that error is not the proof. If the query finishes before the interrupt, the proof fails.

## What this is not

A batch receipt is not an import receipt. PG, Builder, Pack, and OracleSource acceptance stay pending. SP02 HTTP resolve and SP03 stay pending. No grade is promoted.
