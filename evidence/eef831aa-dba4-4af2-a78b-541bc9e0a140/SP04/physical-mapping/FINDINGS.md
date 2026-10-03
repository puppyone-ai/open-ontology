# Findings

The migrations README was left unchanged. It is outside the write set for this task.

`_oo.register_physical_mapping` recomputes SHA-256 and refuses a digest that differs from that result. The live oracles for the empty string, `abc`, 200 bytes of `a`, and the golden namespace payload matched the Rust plan. Lookup reads the stored name. Its `prosrc` has `strpos` 0 for `pm_sha256`.

The stored identifier is 63 bytes: a 3-byte prefix and 60 hex characters, which is 240 bits of the digest. `UNIQUE (identity_digest)` and `UNIQUE (physical_name)` are the collision backstop. The live long pair shared a 63-byte logical prefix and received two different 63-byte physical names.

This module does not run Unicode normalization. Callers pass a finished `pgident/v1` string. `é` and `e` plus U+0301 are one `pgident/v1` value, `q_c3a9`, before they reach the binder. `ﬁ` (`q_efac81`) and `fi` stayed distinct in the live rows, as did `customer` and `q_437573746f6d6572`.

The fixture database is encoding `UTF8`, locale provider `b`, and `datlocale` `C.UTF-8`. On this Postgres 17.10 image, `datcollate` remained `en_US.utf8` for that builtin database. The live assertion reads `datlocale`.

The postmaster check is `docker restart` of this task's Postgres, then a new `psql` session. The golden row matched. A crash kill was not run.

`pg_proc.prosrc` is visible to the superuser catalog query used by the test. `oo_a140_agent` and `oo_a140_runtime` received `42501` on `SELECT` from `_oo.physical_mappings`. Runtime execute of `_oo.pm_sha256` is false. Public execute of lookup is false.

Direct `INSERT`, `UPDATE`, `DELETE`, and `TRUNCATE` on the mapping table returned `OO014`. The golden row was still readable. An unfenced `commit_apply_receipt` returned `OO011`. `apply_receipts` stayed at 0. Public user tables stayed at 0.

`sqlx-postgres` printed two unfulfilled `#[expect(dead_code)]` warnings during Clippy. Clippy still exited 0. Those files were not edited.

`cases.log` appends across runs. The green sequence is the last block and ends with `live=ok`. Earlier consoles are under `linux/fixed/attempt-1` through `attempt-5`.
