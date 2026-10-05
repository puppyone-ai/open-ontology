# Oracles

The integration test compares the child with `guard_select` called in the test process on the same identity, catalog, and SQL. The supervisor library does not call `guard_select`.

Checked on the green run:

- Legal `SELECT`, star expansion, `count`, alias, and literals match the guard, including `original_sha256`, `executed_sha256`, `parser_version`, and physical names.
- `DISTINCT`, `INSERT`, `LIKE`, `INTO`, `FOR UPDATE`, a hidden column, a parse error, and a large-table star return the guard's refusal. Display text for the cost refusal has no `424`. The canary `canary_secret_sql_9f3a` is absent from refusal `Display` and `Debug`.
- Catalog OIDs, aggregate star, operator OIDs, optional literal OIDs, and `null_test_allowed` survive a codec roundtrip.
- Every `GuardError` variant and every allowlisted static label round-trips.
- Identity mismatch is `GuardError::IdentityMismatch` and the display text has no policy id.
- `UNTRUSTED_ADMISSION` is `Blocked`. `RESTRICTION_CAPABILITY` is `Unsupported`. Protocol version is 1. Magic is `OOSW`. Deadline is 500 ms.
- A relative, directory, or missing worker path is rejected before spawn. The approved path is the canonical absolute path of the compiled binary.
- Frames with a bad magic, a bad version, a trailing byte, or a declared length above 8 MiB fail before a large body is kept. A string length of `u32::MAX` returns `CodecError::Limit` without copying that many bytes.
- The production binary with `--slow` or `OO_PARSER_WORKER_SLOW` exits 2 and writes no stdout. A non-request frame and an empty stdin exit 1.
- The slow fault binary is signal 9, reaped, within 500 ms to 3 s, and the pid's start time is gone. The crash fault binary's exit code is 99. The oversize fault binary is signal 9 in under 2 s. Echo is `Malformed` and the canary is absent from the error text. The next delayed child still returns `IdentityMismatch`.
- Two legal calls record two children. `/proc/<pid>/environ` is empty while the parent has `HOME` or `CARGO_HOME`.

Fault binaries are compiled by the test with `rustc --edition 2024`. They are not flags of `oo-sql-parser-worker`.
