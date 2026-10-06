# Findings

Observations from the a249 mapping-reader CLI run. None of these accept a parent plan item or release the gateway.

1. The fixed archive `e0704257a1509b968a80a9ccd2f200319d5483ad` already contains `pub async fn provision_mapping_reader`. Smoke refused an archive that already contained `provision-mapping-reader.rs`. `overlay-absent.txt` is `overlay_absent`. The smoke test was `cargo test -p oo-gateway --lib mapping_reader_access`: 5 passed, 21 filtered out. Lock content hash stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

2. The command is auto-discovered from `src/bin`. `crates/oo-gateway/Cargo.toml` has no `[[bin]]` list, so the new binary did not require a manifest edit. `a207` stayed the sole connector manifest and root lock writer.

3. The policy objects are the reviewed-policy DTO. The offline case `producer_policy_object_reaches_tls` embeds the contract example (`shop`, `clerk`, `clerk_login`, `id` with `pii.class=none`, `email` declared with `explicit_allow`, `row_filter` null) under protocol `oo.gateway.provision-mapping-reader-cli/v1`. Compile succeeded and the private-key CA was refused as `provision-mapping-reader: tls` before a connection. Piping the reviewed-policy protocol string itself is `request`.

4. `install_column_acl_set` still does not grant the login mapping read. After both roles were installed, `mapping_after_column_acl=absent`. `load_policy_catalog` as `oo_a249_login` then returned `CatalogError::Database { sqlstate: "42501" }`.

5. The clerk invocation wrote effect `installed` for role `oo_r_clerk`. The same document again wrote `replay`. The peek document on that same login wrote `replay` for `oo_r_peek`. Role column-grant text was unchanged. Shadow column privileges for the login stayed 0. Mapping-column `SELECT` rows equaled the mapping column count. That listing is the database-wide catalog exposure named in the contract.

6. Stale generation 3 against stored generation 7 exited 7 with `policy stale` and left mapping privileges absent. Unknown login `oo_a249_missing` exited 7 with `policy login`. A second login that held only `USAGE` on `_oo` exited 7 with `policy privilege`; `SELECT` and lookup `EXECUTE` stayed false. The command did not revoke the `USAGE` it had not granted.

7. Compiled queries returned the seeded rows. Clerk customer: 41 `丙号` true, 42 null false, 43 `丁铺` true. Peek orders: 701 with int8 minimum, 702 with int8 maximum, 703 with 0. Source customer count stayed 3. Reader `INSERT` returned `42501`. `PUBLIC` held none of the mapping privileges. The login was not a member of `oo_target_owner`.

8. Offline refusals on the same binary: an argument exits 2 and is not echoed; `{` is scan exit 4; 1048577 bytes is budget exit 3; a wrong protocol and an `audit_file` field are request exit 5; a claims row filter is `policy filter`; an empty policy array is `policy empty`; a missing password file and a symlink password are request; a password whose bytes occur in the document is request; a CA PEM window `PRIVATE KEY` is tls exit 6; a login that differs from the policy login is `policy binding` and does not require the password file to exist.

9. Clippy `-D warnings` on the new binary and `provision_mapping_reader_cli` exited 0 in 1m 27s on the development chain. The chain secret scan was `clean`. A later scan of this evidence tree against the password files and against PEM markers found no password, private key, or certificate. Passwords stayed under `/tmp/oo-a249-eef831aa-dba4-4af2-a78b-541bc9e0a249/secrets` until cleanup removed that directory.

10. The development chain had no failed attempt. Review attempt 1 ran the same 14 tests to `ok` in 2.09s and then exited 1 because `cargo-clippy` was not installed in that new container. The review script omitted the `rustup component add clippy` step the chain already had. Attempt 1 is preserved under `review/attempt-1`. The rerun installed clippy, checked `oo-gateway`, and exited 0. That was a review-harness gap, not a product change.

11. Other tasks had builders and databases during this run. They were not stopped, resized, or deleted. This task's builder count stayed at one, at 6 GiB and 1 CPU. PostgreSQL 17.10 container `99aa39aac9fe45c5f33e75c57be25a0c03ee0057c324b977277a63df3e7e3ca7` had no host port. Cleanup removed only a249 resources. Images were not pruned.
