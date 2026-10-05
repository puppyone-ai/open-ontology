# Parser worker

Contract `oo.gateway.parser-worker/v1`. Library `oo_gateway::parser_worker`. Binary `oo-sql-parser-worker`.

The parent sends one request frame. A fresh child calls `guard_select` and writes one plan frame or one refusal frame. The parent does not parse SQL and does not return the original text as a plan. The wire format is this document. It does not use serde or bincode.

The child is the existing guard. Admission rules, OID checks, and statement limits stay in `contracts/gateway/sql-guard-v1.md`. This contract is the process boundary and the byte layout.

## What this slice does not do

`RESTRICTION_CAPABILITY` is `Unsupported`. `std::process::Command` with `env_clear` is not a kernel network, filesystem, or memory sandbox. This crate does not call `runsc`, does not add a dependency, and does not use `unsafe` to install namespaces, landlock, seccomp, or rlimits. Inherited file descriptors other than the assigned stdin, stdout, and stderr are not closed. A grandchild that leaves the child is not reaped by the direct `SIGKILL`. Those controls remain a separate launcher gate.

`oo_gateway::query_executor::UNTRUSTED_ADMISSION` stays `Blocked`. This module is not an HTTP route, an authentication check, an audit write, a row-level security check, or a database session. The worker receives no database URL, model token, or home directory. Public untrusted admission stays closed until the worker, authentication, audit, and row-level security are all present, including a reviewed production launcher.

`query_executor.rs` is unchanged. Its `UntrustedAdmission::Blocked` comment still says a 500 ms worker is not implemented. That sentence is stale relative to this library. The constant stays `Blocked` for the reason above.

SP06-R8, the public gateway, and the full gateway goal stay open.

## Process

`TrustedWorker::approve` accepts an absolute path to an existing file and stores the canonical path. `AdmitRequest` has identity, catalog, and SQL. It has no path field. The approved path is platform configuration. Caller SQL cannot replace it.

`supervise` and `supervise_with` start one child per call. The child is not reused. `on_start` runs after `spawn` and before `wait`, so `/proc/<pid>` still names that child on Linux, including a zombie. The next call does not signal a pid from an earlier call. `Child::kill` sends `SIGKILL` to that child. The parent then `wait`s. It does not signal a process group.

The command is the canonical path. There is no shell. The environment is cleared. The working directory is an empty directory under the temp dir, named `oo-parser-worker-<parent pid>-<counter>`, removed when the call returns. Standard input and output are pipes. Standard error is null. On Unix the child is placed in a new process group (`process_group(0)`).

The deadline is 500 ms from immediately before `spawn` through the reply read (`PARSE_DEADLINE`). A frame that arrives at or after that instant is a timeout. Timeout, oversize, malformed, crash, and cancel all `SIGKILL` and `wait`, except cancel that fires before `spawn`, which returns `Cancelled(None)` and starts nothing. An encode failure is `RequestBudget` and starts nothing.

A usable reply is exit status 0 and exactly one plan or refusal frame, with the following read returning end of file. A non-zero exit is `Crashed` even when a frame was decoded. End of file before a frame is `Crashed`. A bad magic, version, kind, or body is `Malformed`. A declared body longer than 8 MiB is `Oversize` and the body is not allocated.

The parent reads the reply on a dedicated thread. It does not call `Read::read_to_end` on the child, and it does not use `tokio::task::spawn_blocking` or `JoinHandle::abort` as the kill.

## Child

`oo-sql-parser-worker` reads one request frame, drops stdin, calls `guard_select`, and writes one frame.

| Exit | When |
|---|---|
| 0 | One plan or refusal frame was written and flushed. |
| 1 | The frame or the reply encode failed. No reply is promised. |
| 2 | Any argument is present, or `OO_PARSER_WORKER_SLOW` is set. Stdout is empty. |

`OO_PARSER_WORKER_SLOW` is not a slow path. The production binary has no fault flag. Slow, crash, and oversize programs exist only as test binaries compiled by `tests/parser_worker.rs`.

## Frame

Little-endian. No padding. Lengths are byte lengths.

| Offset | Field |
|---|---|
| 0 | 4 bytes ASCII `OOSW` |
| 4 | `u16` version, must be 1 |
| 6 | `u16` kind |
| 8 | `u32` body length |
| 12 | body |

The buffer length must equal 12 plus the body length. A longer buffer is trailing and is rejected. Version other than 1 is rejected before the body is decoded. A body length above 8 MiB (`MAX_FRAME_BODY`) is rejected before the body is allocated.

| Kind | Value | Direction |
|---|---|---|
| request | 1 | parent to child |
| plan | 2 | child to parent |
| refusal | 3 | child to parent |

`decode_frame` checks magic, version, length, and trailing bytes. The parent accepts only kinds 2 and 3 from the child. The child accepts only kind 1. Any other kind is rejected before the body is copied into the error.

## Common encodings

A string is a `u32` byte length and then that many bytes. The bytes must be UTF-8. If the length is above the field cap, the decoder returns `Limit` and does not copy the bytes.

A list is a `u32` count and then that many items. If the count is above the field cap, the decoder returns `Limit` and does not reserve the items.

A flag is a `u8`: 0 false, 1 true. Any other byte is `Tag`.

An optional `u32` or string is a `u8`: 0 absent, 1 present and followed by the value. Any other byte is `Tag`.

An unknown tag or a static label outside the lists below is `Tag` or `Label`. Bytes left after a complete value are `Trailing`. A buffer that ends inside a field is `Truncated`.

The codec checks sizes, counts, UTF-8, tags, and the static labels. Identifier grammar stays in the guard, which runs after the child decodes the request. A refusal body has no SQL text and no privilege or grant bit.

## Request body

| Field | Cap |
|---|---|
| project, role, policy id | 128 bytes each |
| catalog, same layout as below | |
| sql | `SQL_BYTE_BUDGET` (65536) |

Catalog:

| Field | Cap |
|---|---|
| identity | same three strings |
| relations | 128 |
| functions | 512 |
| operators | 512 |
| literal integer, text, boolean | optional `u32` each |
| null_test_allowed | flag |
| large_table_row_threshold | `u64` |

`VisibleRelation`: schema, name, physical schema, physical name (each ident, 63 bytes), columns (256), `pending_unclassified` `u32`, `estimated_rows` `u64`.

`VisibleColumn`: name, physical name (ident), `type_oid` `u32`.

`ResolvedFunction`: `oid` `u32`, schema, name (ident), argument type OIDs (100 `u32`s), `aggregate_star` flag.

`ResolvedOperator`: `oid` `u32`, `procedure_oid` `u32`, schema, name (ident), `left_type_oid` `u32`, `right_type_oid` `u32`.

## Plan body

| Field | Cap |
|---|---|
| identity | 128 bytes each string |
| original_sql | 65536 |
| executed_sql | 1 MiB |
| original_sha256, executed_sha256 | 64 bytes each |
| fingerprint_hex | 128 |
| parser_version | `i32` |
| star_expansions | `NODE_BUDGET` (10000) |
| bindings | below |

`StarExpansion`: qualifier (256 bytes), schema, name (ident), columns (`NODE_BUDGET` idents).

Bindings: relations, outputs, functions, operators, each capped at `NODE_BUDGET`, then `null_tests` `u32`.

`BoundRelation`: schema, name (ident), alias (optional ident), physical schema, physical name (ident).

`OutputItem` is a `u8` tag:

| Tag | Payload |
|---|---|
| 1 column | relation schema, relation name, name, physical name, output name (idents), `type_oid` `u32` |
| 2 function | bound function |
| 3 literal | literal tag |
| 4 compound | no payload |

`BoundFunction`: `oid` `u32`, schema, name (ident), `aggregate_star` flag.

`BoundOperator`: `oid` `u32`, `procedure_oid` `u32`, schema, name (ident).

Literal tags: 1 integer, 2 text, 3 boolean, 4 null.

## Refusal body

One `u8` tag, then the payload. The original SQL is not a field.

| Tag | Variant | Payload |
|---|---|---|
| 1 | IdentityMismatch | none |
| 2 | CatalogInvalid | static catalog reason |
| 3 | Parse | none |
| 4 | Budget | none |
| 5 | Statement | none |
| 6 | Write | none |
| 7 | Into | none |
| 8 | Locking | none |
| 9 | SystemSchema | none |
| 10 | Unsupported | static construct |
| 11 | Serialize | static serialize reason |
| 12 | FunctionNotAllowed | none |
| 13 | RelationNotFound | none |
| 14 | ColumnNotAllowed | string list (512 labels, 256 bytes each), `pending` `u32`, `ambiguous` flag |
| 15 | Unqualified | optional suggestion, 256 bytes |
| 16 | CostTooHigh | relation string, 256 bytes |

A static label is a string of at most 64 bytes that is exactly one entry in the list for that variant. The decoder returns the `'static` string from that list.

Catalog reasons: `duplicate column`, `duplicate function`, `duplicate operator`, `duplicate relation`, `empty column list`, `function oid`, `function schema`, `identifier`, `identity`, `operator name`, `operator oid`, `physical schema`, `relation schema`, `type oid`.

Serialize reasons: `deparse`, `fingerprint`, `idempotent`, `reparse`, `recheck`, `second deparse`, `version`.

Constructs: `ANY predicate`, `BETWEEN predicate`, `CASE`, `CTE`, `DISTINCT`, `GROUP BY`, `HAVING`, `IN predicate`, `JOIN USING`, `LIKE predicate`, `LIMIT WITH TIES`, `NATURAL JOIN`, `ONLY`, `VALUES`, `WINDOW`, `aggregate star`, `ast node`, `bit string`, `boolean`, `cast`, `catalog qualifier`, `column alias list`, `column reference`, `duplicate range name`, `empty target list`, `expression`, `float literal`, `from item`, `function`, `function clause`, `identifier`, `join`, `join alias`, `join type`, `limit`, `literal`, `null test`, `operator expression`, `order key`, `order nulls`, `order operator`, `output alias`, `parameter`, `parser version`, `predicate`, `range function`, `relation persistence`, `set operation`, `star`, `star alias`, `subquery`, `table function`, `tablesample`, `target`, `target indirection`, `unary operator`.

## Display

`CodecError` and `SuperviseError` display text is fixed. It does not include the SQL, the catalog, or a path. `GuardError` display stays the guard's text. Hidden names and parser text are not added by this codec.
