# source-capture-policy v1

Capture planner for one declared source. Contract name: `source-capture-policy/v1`.

The program reads one JSON request and writes one JSON decision. It chooses
a capture method, a consistency grade, retention action, cursor and delete
quality, and whether a position may be repaired or advanced. It does not
start a scheduler, write the loader, publish a generation, or open a
database.

`loader_integration` and `scheduler_integration` are `unaccepted`.
`live_offset_mirror` is `not_run`. A decision is not an applied watermark
and it is not an acknowledgement.

## Dependencies

Path dependencies are the fixed crates at the dependency commit, not stubs:

- `cdc-spool` `recover` reads a sealed transaction directory.
- `cdc-offset-store` `receipt_hash`, `checkpoint`, `cmp_pos`, and `pos_ok`
  decide a supplied receipt. `receipt_closed` is implemented in that crate
  and is not part of its public exports, so this planner does not call it
  by name. `checkpoint` returns `receipt_rule` when that check fails and
  returns `input` only after it passes. `mirror` is not called. That call
  needs a target database and a control database; this planner does not
  open either.

A sealed spool never authorizes ACK, target apply, or cross-table
consistency. Those spool attestations stay false. A rolled-back, incomplete,
unknown, or unreadable spool is not a commit position. If the sealed
commit position and the offset receipt position differ, repair and advance
are refused. The planner does not take the greater position.

## Command

`source-capture-policy plan <request.json>`

The file must be a regular file of at most 1 MiB. Unknown JSON fields are
refused. Object keys named `password`, `passwd`, `secret`, `credential`,
`credentials`, `api_key`, `apikey`, `token`, `access_token`, or
`refresh_token` are refused. The refusal body does not echo the value.
The C2 snapshot identity is the field `snapshot_token`, so a real request
does not use the refused key `token`.

Exit 0 is a decision, including a decision that denies the request.
Exit 2 is a request the planner could not read.
Exit 1 is usage or a local I/O failure.

`downgraded` is false. An explicit method that fails its gate is refused.
Auto does not switch to another method in that case. Auto, when selected,
considers only `verified` or `certified` methods, in this order: C1, C2,
C4, C3, C5.

## Decisions

C1 requires grade T, a transaction boundary, token order Total, and write
class R0 or R1. C2 requires a token and an `AS OF` read strictly shorter
than token retention. An S1 read longer than 300 seconds is refused. C3
requires a cursor class. `statement_time` needs overlap at least the maximum
transaction age and a reconcile configuration. `app_maintained` is not
chosen by auto. `coarse` is never the cursor. A truncated sub-microsecond
column is not a cursor or primary key unless the original column is kept as
the key. Equal cursor values require a `(run_generation, cursor, pk)`
checkpoint. C4 requires trusted partition metadata. C5 above the row limit
is refused. A table with no stable or declared unique key is only C5, or
C4 full-partition replay.

`transactional` is accepted only when every selected table is grade T and
its position is comparable. It is denied for S1, S0, and B. The planner
does not rewrite that request as `per_table`.

Repair and a consistency claim require token order Total, a position inside
the window, and, when an offset receipt is present, a real `insert_first`
or `compare_and_swap` from `cdc-offset-store`. `PerPartition`, `Opaque`,
and `None` cannot be repaired. `mariadb-gtid/v1` is comparable for family
`mariadb`, method C1, a known transaction boundary, and one stored identity
`d{domain}.s{server}` plus that sequence. A set, another domain, another
server, or a binlog file position is not that identity. Family `mariadb`
with `mysql-binlog-pos/v1` is `binlog_pos_not_durable`. `mysql-gtid/v1`
stays `position_compare_unimplemented`: same-source UUID GTID set order is
not implemented, and MariaDB domain-server rules are not applied to it.
`mysql-binlog-pos/v1` is comparable only for the same MySQL instance and is
expired after failover.

When an offset block is present, each planned table is bound on its own.
Its generation must be the selected generation or listed in
`required_table_generations`, and a receipt row for that generation must
be present. That row must match the selected receipt on source, source
epoch, position codec, position lineage, applied position, and the config,
type, hook, schema, and payload digests. The selected method must use that
same epoch, codec, lineage, and applied position. Comparison is string
equality, which is the sibling rule inside `cdc_offset_store::checkpoint`.
Positions stay decimal strings. This crate does not parse them as
floating point. `checkpoint` requires equal `applied_pos` across required
siblings, so a fully bound set shares one applied position.

A checkpoint names one selected receipt. It does not authorize another
table. Repair, advance, continue, and a consistency claim are per table.
`same_instant_claim_allowed`, `transactional_allowed`, source
`repair_allowed`, and source `advance_capture_position` are true only when
every planned table is bound and verified. An unbound generation, a missing
required receipt, or a conflicting sibling fails closed. So do a source id,
epoch, digest, schema, or position mismatch. The source operator is then
`refuse`. A join of two or more tables warns `CROSS_TABLE_SKEW` when any
participant cannot claim consistency.

Retention uses integer ratios. Lag greater than half the window is
`STALE_RISK`. Lag greater than four fifths pauses publication of a new
generation. Lag beyond the window, a decimal position outside the declared
bounds, or an unproven failover is `POSITION_EXPIRED`. The action is a new
generation when the current name ends in `_gN`, the old generation stays
readable, and the old position is not advanced. The planner does not
continue from the newest source position.

Delete detection `reconcile` sets `DELETES_LAGGED` when the reconcile
period is within the visibility SLA. A longer period is refused. `none`
sets `DELETES_UNDETECTED`. A soft-delete column does not count as physical
delete detection. A join of two tables is `CROSS_TABLE_SKEW` when the
sources differ, any table is below T, or any participant cannot claim
consistency. Two grade T tables in one source do not get that warning when
each consistency claim is allowed. S1 tables can share one token for a
snapshot claim and still warn on the join.

`_oo.table_quality` is not migrated here. The decision lists the status
values the loader would have to record. Budget exhaustion is a supplied
flag, not a measured cloud bill.

`rust-version` is 1.94.0. Edition 2024. Safe Rust only. serde 1.0.229 and
serde_json 1.0.151 match the CDC crates.
