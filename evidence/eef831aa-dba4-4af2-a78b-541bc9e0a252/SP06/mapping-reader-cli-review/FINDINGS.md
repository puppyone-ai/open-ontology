# Findings

Observations from the a252 review of frozen commit `9df074813572569950d85bbe951069b4db83facc`. None of these accept a parent plan item or release the gateway.

1. The a249 `review/REPORT.md` is a same-session review. It reused that task's PostgreSQL container and cargo volume, and its second clippy finished in 0.48s because the dependency graph was already built. This session did not use those resources. Smoke container `706134e1d91f991dbe9eeb6d576da89a3c3fb3fb9540030871db9e6ec530d719` and proof container `1c507107d9b65aea0fae4ec879e4c772effdb9ad48240368f544202d75fde595` compiled from a new partial archive of the fixed commit.

2. The fixed commit already contains `provision-mapping-reader`. Smoke refused to continue only if that file was absent. `overlay-absent.txt` is `overlay_absent` in both phases. With `OO_A249_LIVE` unset, the committed live test returned before connecting: 14 passed in 0.01s, and no oracle was written. Lock content hash stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

3. The command is auto-discovered from `src/bin`. `crates/oo-gateway/Cargo.toml` has no `[[bin]]` list. The fixed commit does not edit `lib.rs`, `mapping_reader_access.rs`, `policy.rs`, SQL, manifests, or `Cargo.lock`. `a207` stayed the sole connector manifest and root lock writer. `cargo update` was not invoked.

4. `compile_column_acl_set` runs before a password file or CA file is opened. A login that differs from the compiled policy login is `policy binding`. Mixed identity is `PolicyError::MixedIdentity` inside that compiler, which the command maps to `policy identity`. The offline producer case embeds the reviewed-policy object (`shop`, `clerk`, `clerk_login`, `id` with `pii.class=none`, `email` declared with `explicit_allow`, `row_filter` null) under protocol `oo.gateway.provision-mapping-reader-cli/v1` and stops at `provision-mapping-reader: tls` on a CA whose PEM window is `PRIVATE KEY`.

5. `install_column_acl_set` does not grant the login mapping read. After both roles were installed, `mapping_after_column_acl=absent`. `load_policy_catalog` as `oo_a249_login` then returned `CatalogError::Database { sqlstate: "42501" }`.

6. The clerk invocation wrote effect `installed` for role `oo_r_clerk`, with no trailing newline and empty stderr. The same document again wrote `replay`. The peek document on that same login wrote `replay` for `oo_r_peek`. Role column-grant text was unchanged. Shadow column privileges for the login stayed 0. Mapping-column `SELECT` rows equaled the mapping column count. The import also stored `private_note` and `ratio`, which are outside the clerk allowlist. The listing is database-wide. This proof does not `SELECT` those mapping rows. It is not a tenant boundary.

7. Stale generation 3 against stored generation 7 exited 7 with `policy stale` and left mapping privileges absent. Unknown login `oo_a249_missing` exited 7 with `policy login`. A second login that held only `USAGE` on `_oo` exited 7 with `policy privilege`; `SELECT` and lookup `EXECUTE` stayed false. The command did not revoke the `USAGE` it had not granted.

8. The committed assertions passed for the seeded rows. Clerk customer: 41 `丙号` true, 42 null false, 43 `丁铺` true. Peek orders: 701 with int8 minimum, 702 with int8 maximum, 703 with 0. Source customer count stayed 3. Reader `INSERT` returned `42501`. `PUBLIC` held none of the mapping privileges. The login was not a member of `oo_target_owner`. `oo_pipeline_runtime` was not a member either. The outcome JSON field `source_grant` is a constant `false` written by the test. The measured source refusal is `source_reader_insert=42501`.

9. The frozen test hardcodes host `oo-a249-pg`, database names `oo_a249_src` and `oo_a249_dst`, and gate `OO_A249_LIVE`. Those strings stayed in the committed test. The cluster that served them is container `oo-a252-pg` on network `oo-a252-eef831aa-net`, with alias `oo-a249-pg` and SAN `DNS:oo-a249-pg`. No a249 container, volume, network, or secret directory was reused. The rust builder's secret mount was `ca.crt` and six password files. `server.key` and `ca.key` were not in that mount.

10. `from_connect` maps `PolicyError::Database` to `tls`. `from_mapping` maps `MappingReaderError::Database` to `policy database`. A connection failure is therefore `tls`, and a database error after the session is open is `policy database`. The contract's exit 6 is the CA or the TLS session. This proof's successful path used a verified private CA and did not exercise a bad password.

11. Clippy `-D warnings` on the binary and `provision_mapping_reader_cli` exited 0 in 1m 26s. The secret scan was `clean`. A scan of the live outputs for `PRIVATE KEY` was `clean`. Passwords stayed under `/tmp/oo-a252-eef831aa-dba4-4af2-a78b-541bc9e0a252/secrets` until cleanup removed that directory.

12. Smoke and the live proof each exited 0 on the first run. There is no failed attempt to preserve. `oo-a251-preoverlay` was running when the proof builder started. It was not stopped, resized, or deleted. This task's builder count stayed at one, at 6 GiB and 1 CPU. PostgreSQL stayed at 1 GiB and 1 CPU with no host port.
