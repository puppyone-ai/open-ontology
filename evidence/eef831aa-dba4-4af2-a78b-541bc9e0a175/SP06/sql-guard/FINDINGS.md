# Findings

Source commit `03512f91324ac33e772e9702fe806e8e04037ec4`. The deciding compile is `linux/compile.exit` = 0. The deciding database comparison is `live/oracle.txt` and `linux/sql-guard-live.log`.

## F1. Parser pin and native build

`oo-gateway` depends on `pg_query` `=6.2.1` from crates.io. Checksum `da43168013d137b3ca49b0044c1108ec09ff596a0626d4db89209c0a1d450cd2`. The crate manifest license is MIT. Vendored PostgreSQL sources are the PostgreSQL license. The rest of libpg_query is BSD-3-Clause. `ci/deny.toml` has no PostgreSQL SPDX entry and was not edited. `cargo deny` was not run. This pin is not a supply-chain release approval.

The Rust image had no `libclang` and no `pkg-config`. `apt-get` failed with exit 100 because cap-drop `ALL` cannot switch to `_apt` (`linux/apt-update.log`). No capability was added to the builder. Eight Debian trixie arm64 debs were extracted and checked (`linux/libclang-debs.txt`): `libclang-19-dev`, `libclang1-19`, `libobjc-14-dev`, `libclang-common-19-dev`, `libllvm19`, `libobjc4`, `libz3-4`, and `libgc1`, LLVM 19.1.7. `/tmp` on the container is `noexec`, so the tree was copied to `/cargo-home/a175-clang-root`. `LIBCLANG_PATH` and `LD_LIBRARY_PATH` point at that tree. `pkg-config` still reports that `libclang` was not found. `ld-linux --list` resolved `libLLVM.so.19.1` and `libz3.so.4` (`linux/libclang-ldd.txt`).

bindgen 0.66.1 does not find the LLVM 19 resource directory from the real `libclang-19.so.19` path. `BINDGEN_EXTRA_CLANG_ARGS` adds `--target=aarch64-unknown-linux-gnu` and the include directories for the clang 19 resource dir, gcc 14, and `/usr/include`. The first build failed on `stdbool.h` (`linux/lib-test-stdbool.log`). The green build used those flags.

`protoc` is absent (`linux/protoc.txt`). The published `protobuf.rs` in the crate was used. Product code and the SQL guard test contain no `unsafe` block. `crates/oo-gateway/src/lib.rs` keeps `#![forbid(unsafe_code)]`. The FFI stays inside `pg_query`.

`Cargo.lock` gained 35 name/version pairs and lost none. Checksums of packages that were already locked did not change. New pairs include `pg_query` 6.2.1, `prost` 0.13.5, `bindgen` 0.66.1, `clang-sys` 1.9.1, `thiserror` 1.0.69, and `itertools` 0.10.5. Root `Cargo.toml` was not edited. The architecture gate was not edited. `oo-arch-check` printed `architecture check: pass`.

## F2. What the guard does

`guard_select` requires the caller's `PolicyIdentity` to equal the catalog identity. It parses with `pg_query::parse`, walks every protobuf node variant shipped in 6.2.1, admits one plain `SELECT`, rewrites logical names to the approved physical names, expands stars to visible columns in catalog order, deparses, reparses, and requires the second deparse to match. A second admission on the physical names must reproduce that SQL and expand no star. Failure of that chain is `Serialize` with a static reason. The original SQL is not returned as executed SQL. Parser text is dropped on `Parse`.

The supported shape is one `SelectStmt`: schema-qualified base relations, comma and `INNER`/`LEFT`/`RIGHT`/`FULL`/`CROSS` joins, aliases, visible stars, `count(*)` when one snapshot OID matches, typed comparisons, boolean literal casts, `AND`/`OR`/`NOT`, null tests when the snapshot allows them, `ORDER BY` a column, and non-negative integer `LIMIT`/`OFFSET`. `WITH`, subqueries, set operations, `NATURAL`, `USING`, general casts, parameters, `LIKE`/`IN`/`BETWEEN`, writes, utilities, and system schemas fail closed.

Hidden and missing relations share `RelationNotFound`. Hidden and missing columns share `ColumnNotAllowed` and the message lists visible labels only. A 64 KiB statement is admitted; one more byte is `Budget`. A flat `AND` list of 2500 comparisons is `Budget`; 2000 is admitted. The test uses 2799 extra `AND 1=1` terms and expects `Budget`.

## F3. Depth budget and prost

`DEPTH_BUDGET` is 128 and the walker returns `Budget` when a decoded node is deeper than that. `pg_query::parse` decodes with prost's default recursion limit. A chain of 40 `NOT` operators decodes and is admitted. A chain of 48 returns `Parse` (`recursion limit reached`) before the walker runs. The 140-`NOT` fixture in `linux/sql-guard-test-depth.log` failed for that reason. The contract states this. The 500 ms worker isolation is not implemented.

## F4. Live comparison

The test, acting as the future adapter, read `pg_type`, `pg_attribute`, `pg_operator`, and `pg_proc` and put those OIDs in `PolicyCatalog`. The guard did not query PostgreSQL.

The installed column role executed the guarded star and the guarded `order_id = 42` filter. Results were 42 and west. The physical names in the executed SQL are the shadow names. The secret column name is absent from that SQL.

Unguarded `SELECT * FROM logistics.shipment` with an empty `search_path` returned `42P01`. The logical name is not installed as a table. This run does not show `42501` for that statement.

Guarded `count(*)` returned one row (`ok:1`). That execution does not prove a column `GRANT` denied or allowed `count(*)` in general. The contract leaves `42501` as a possible later outcome.

## F5. What stays open

SP06-W02 stays open. Authentication, token exchange, HTTP, portals, pools, SQL execution inside the library, row-level security, the audit WAL, and publication of a read API are open. The guard has no executor.

Also open, and refused when the SQL contains them: CTE scope, subqueries, `LATERAL`, set operations, `GROUP BY`, `DISTINCT`, windows, `NATURAL`/`USING`, `LIKE`/`IN`/`BETWEEN`, general casts, parameters, float literals, and `ORDER BY` an output alias. Parser process isolation and the 500 ms kill are open. `pg-policy-v1.md` still describes the grant slice and was not edited; it still says the grant contract does not link a parser. This contract is the one that links `pg_query`.

## F6. Preserved failures

Apt, the absent-package update, the illegal `cargo fetch -p`, the missing `stdbool.h`, the string comparison, the depth fixture, the first clippy run, and the duplicate derive are the files in the report's failed-attempt table. The green compile log is `linux/compile-run.log`.
