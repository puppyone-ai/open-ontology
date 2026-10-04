# Findings

## PostgreSQL 17.10 renders boolean text as false

`rolcanlogin::text` and the other role flags came back as `false`, seven times per role. The first live assertion expected `f`. That was attempt 2, live exit 101, after a successful bootstrap. The harness now expects the text this server returned. The role attributes themselves are the `0000` attributes: `NOLOGIN`, `NOSUPERUSER`, `NOCREATEDB`, `NOCREATEROLE`, `NOINHERIT`, `NOBYPASSRLS`, `NOREPLICATION`.

## Clippy held two library nits

Attempt 1 compiled the library tests (9 passed, including both installer unit tests) and then Clippy denied `dead_code` on `ABSENT_MESSAGE` and `collapsible_if` in the checksum comparison. The constant is now the guard that turns server `OO002` into `MissingRelation`. The comparison is one `if let` with a conjunction. The green Clippy run is exit 0.

## Lock bookkeeping stayed inside one package

`cargo fetch --locked` succeeded against the hand edit. The diff is four names on `oo-pipeline` in `Cargo.lock`. Dev-dependencies are not repeated there. No package version, checksum, or feature line elsewhere in the lock was rewritten.

## Roles are cluster-wide

`0000` was not edited. The live harness drops `oo_a150_runtime`, then `oo_pipeline_runtime`, then `oo_target_owner` between fixture databases, after `DROP DATABASE ... WITH (FORCE)`. A second empty database on a cluster that still has those roles is not the fresh route. The README and `contracts/pipeline/target-installer-v1.md` say so.

## Mapping-only install was not a live database

The route that runs `0004` alone, when v3 is already valid and `physical-mapping/v1` is absent, is in `route_for` and in the contract. The required live fixtures were empty install plus replay, direct v1, retained v2, `OO002`, `OO001`, the `OO021` rollback, and runtime verify-without-install. Those passed. There is no seventh database that starts from a valid v3 row and installs only `0004`. The route unit test does not construct that catalog either.

## Readiness after failure

`OO001` left the lie checksum and inserted no mapping row. `OO002` left the pre-drop migration text and reported incomplete readiness. `OO021`, from a receipt that violates the closed segment rule, left that receipt and the v1 markers, inserted no v3 row, and reported incomplete readiness. The runtime member of `oo_pipeline_runtime` received `VerifiedReadiness` and `InstallError::NotAdministrator`.

## Vendor warnings stayed in the dependency

`sqlx-postgres` printed two unfulfilled `dead_code` expectations during the package build. They were warnings on the dependency. `cargo clippy -p oo-pipeline --all-targets -- -D warnings` still exited 0. Those vendor lines were not edited.
