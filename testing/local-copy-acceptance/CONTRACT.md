# local-copy-acceptance/v1

Independent judge for MP-01a–d. This crate does not copy a database, does not
open a PostgreSQL socket, and does not sign a missing producer.

Owner `eef831aa-dba4-4af2-a78b-541bc9e0a409`. Writable trees are this directory
and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a409/SP03/local-copy-acceptance/`.
`tools/local-pg-mirror`, source crates, the 405 tree, and the product root are
read-only archives.

`customer_complete` is always false. `enterprise` is `not_configured`. A
signature is `mp01_signed` and is only the public-fixture chain below. It is
not an enterprise copy, not CDC `live`, and not MP-02–06.

## Frozen inputs

Protocol `oo.acceptance.local-copy/v1`. Unknown keys, duplicate keys, and keys
named `password`, `passwd`, `secret`, `authorization`, `credential`,
`private_key`, `ca_pem`, `ca_body`, `api_key`, or `token` are refused.
`customer_complete: true` is refused. JSON numbers are integers.

The short goal dated 2026-10-08 20:07:59 keeps the SRC-D01 ceilings. A value
outside these ranges fails the card:

- `batch_rows` 1..=1000
- `statement_timeout_ms` 1..=30000
- `lock_timeout_ms` 1..=1000
- `snapshot_deadline_ms` 1..=300000

Budgets are not widened by a manifest field. `run` returns before spawn when
a ceiling is broken.

## Cards

`checks_pass` is the mechanical result of one card. `mp01_signed` is true only
when every card passes and `provenance` is `live`. `mp01abc_signed` is the
copy partial signature: MP-01a, MP-01b, and MP-01c pass on a live bundle.
MP-01d can stay pending for the 405 and 397 consumer without withholding that
copy claim. `consistency_claim` is `single_snapshot` when `mp01abc_signed` is
true. `fixture` and `archive` add `provenance_not_live` and stay unsigned. A
passing MP-01a on a one-row bundle does not sign the chain.

MP-01a. Source and target `system_identifier` values are non-empty and
different. Two databases on one postmaster share that identifier and fail
`fingerprint_same_instance`. The producer owner is
`eef831aa-dba4-4af2-a78b-541bc9e0a408`. A live signature also requires
`binary_present`, a 64-character lowercase SHA-256, `invoked`, exit 0,
`fixed_single_connection`, `source_socket_fact` `repeatable_read_read_only`, and
`source_socket_peak` 1.

MP-01b. Business peak is the maximum, across samples, of connections whose
class is `business` or `guard`. Class `monitor` is the external fixture-admin
session. It is echoed in `connections` and stored as `monitor_peak`. It is not
added to the business peak. Attributes on every connection are `class`,
`usename`, `datname`, `application_name`, `backend_type`, `xact_isolation`,
`read_only`, `state`, and `pid`. A `verified_reader_guard` fact with no guard
row in a live or fixture sample is `guard_hidden`. A guard row, or that fact,
is `guard_not_a_single_connection` and counts toward the peak. The passing
shape is one `business` backend, `repeatable read`, `read_only`, and no guard
backend. Catalog and data snapshot ids are equal and non-empty. `proof` is
`sealed_spool`. `later_count` and `opened_second_source_connection` fail the
card. Source `ddl`, `dml`, `heartbeat`, and `privilege` are `source_write`.
`explain` and `count_star` fail on their own. The selected counts are exactly
2000, 8000, and 90000. The sealed rows and the target rows must include int8,
decimal, null, time, and binary. Equal counts without an equal canonical hash
fail `typed_mismatch`.

MP-01c. The bundle contains `partial`, `cancel`, `snapshot_disconnect`,
`restart`, `idempotency`, and `mixed_version`. Each stays `public` false,
`mirror_state` other than `complete`, `consistency` `not_a_snapshot`,
`mixed_snapshot` false, `target_rows_rewritten` false, and
`source_backends_after` 0. Those outcomes do not become a public copy.

MP-01d. `fixture_class` `public` has the public fixture source blocked,
`network_to_source` false, and the source secret absent from the subsequent
process. `enterprise_stop_requested` fails `enterprise_stop_refused`. This
program has no stop command; `issued_stop` is false. An enterprise bundle
cannot be signed by blocking that source. Audits contain one `405` and one
`397`, the same non-empty generation, `target_instance` equal to the target
`system_identifier`, `database` equal to the target database, a non-empty
`query_id`, and `source_access_count` 0. MP-02–06 stay with owner
`eef831aa-dba4-4af2-a78b-541bc9e0a407`. `repeated_here` true fails
`repeated_campaign`. A reused artifact hash is recorded and is not re-run here.

## Typed preimage

UTF-8, columns sorted by name, rows sorted by key:

```
local-copy-acceptance/v1 table
schema=<schema>
name=<name>
keys=<k1>,<k2>
count=<n>
---
<col>=<cell>\t...
```

Cells: `int8:<canonical i64>`, `decimal:<scale>:<text>` with exact scale,
`null:`, `time:<source text>`, `binary:<lowercase hex>`, `text:<escaped>`,
`bool:true` or `bool:false`. Decimal scale 0 has no dot. Scale 1..=38 has
exactly that many fraction digits. Negative zero is refused. int8 text is the
canonical decimal, including `9223372036854775807`. Time is
`YYYY-MM-DD[ T]HH:MM:SS` with an optional 1..=6 digit fraction and an optional
`Z` or `±HH:MM`, or a time-of-day `HH:MM:SS` with an optional 1..=6 digit
fraction and no zone. The judge does not invent a date and does not convert
zones. SHA-256 of the preimage for
`public.amounts`, key `id`, cells `id=int8:1` and `note=null:` is
`667f23b6789a9affd9ea6d54b404e2ee3fecbd6b61ac6b12cee8e0e84d8877fd`.

Spool input is JSONL at an absolute regular file. A symlink is refused. Inline
rows and spool paths are not combined. The judge hashes those bytes. It does
not open a second source connection to recompute them.

## Run

`local-copy-acceptance judge <case> <verdict>` writes a verdict and exits 0
even when the verdict is withheld. `run <binary> <case> <verdict>` never sets
`mp01_signed`. It spawns the binary only after the ceilings, the
single-connection pin, a regular-file SHA-256 match, and absolute argv paths
all hold. Otherwise it exits 4 and does not spawn. A spawn exits 5 and still
does not sign; signing remains a later `judge` of the measured bundle.
`runner_source_sockets` is 0 because this process opens no source socket. That
constant is not a measurement of the producer.

## Archive this session judges

`fixtures/archive-unfixed.json` records the read-only `tools/local-pg-mirror`
archive: no a408 binary, `source_socket_fact` `verified_reader_guard`,
`source_socket_peak` 2. That bundle stays unsigned. A unit test can sign an
in-memory bundle that already contains every measured input. That test does
not read a database and is not this session's signature.
