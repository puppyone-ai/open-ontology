# Oracle

The independent hash check is `assert_rust_sha256` in `crates/oo-builder/tests/role_pack.rs`. It parses the pack artifact, checks that the artifact bytes are canonical JSON, recomputes each file SHA-256 and byte length, recomputes the pack digest from those records, recomputes `cache_key_sha256` from the cache key bytes, and recomputes `manifest_sha256` from `manifest.json`.

The digest is SHA-256 from the `sha2` crate, version 0.10.9, already listed as a dev-dependency of `oo-builder`. The test file has its own `sha256_hex` and canonical JSON writer. It does not call the library helper. No crate was added. This task did not edit `Cargo.lock`.

`escaped_sql_label_distinct_and_name_closure` calls that check for the clerk pack and for the same artifact under principal `ada2`. `hashes_match_an_independent_oracle` is the older analyst check and still uses the same helpers. Both passed on `linux/commit` (`test.exit` 0, 11 passed) and on `linux/focused-2` (`test.exit` 0, 11 passed). `linux/commit` is the run against commit `eff1959b` and lock content `45a784dc73ee1ba7531430bb4f32f368f22220271c28fccb84540593953bcffd`. `linux/focused-2` used the `ef34ad7b` lock content `34b7364c3c68b48a7d1c1cab40b659408ce9087f14a15626c38ed2016a63af8e`. `sha2` 0.10.9 is in both locks.

No separate JSON digest file is emitted. The pass is the comparison inside the test process. The raw log is `linux/commit/test.stdout` and `linux/commit/test.stderr`.

The a168 review hashed packs with Python `hashlib` (`overlay/oracle.py` and `linux/probe-3/out/python-oracle.json` under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a168/SP07/role-pack-review/`). That tree is a historical record of the review. This task left those files in place and did not run Python.
