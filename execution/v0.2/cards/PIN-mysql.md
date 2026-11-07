## Card PIN-mysql — pinned P0 re-measurement: mysql

TODO: S5-H-01 (closes re-review M2/N1/m5). Your worktree is a DETACHED-equivalent branch `v0.2/pin-mysql` at commit
d5e9653c9ff96fe6e3d37dc4b6bc89d98162a8c6. Do NOT commit, rebase, or edit any tracked file: every evidence row records
`product_sha` from HEAD (with `-dirty` if tracked files differ) and must equal that commit exactly. If a harness or
product defect blocks a suite, stop that suite, record the failure, and report it — do not fix it here.
Build first (dispatch deletes Cargo targets): `cargo build -p oo-cli --locked`;
`cargo build --manifest-path tools/oo-load/Cargo.toml --target-dir target --locked --features mysql-binlog`.
Oracle 11 needs the thick worker by image content ID: `docker image inspect -f '{{.Id}}' oo-e-oracle-worker:e-fix-worker-v4`
(set `OO_TEST_ORACLE_WORKER_IMAGE` / `--worker-image`); Oracle 19c T7 goes through `testing/engines/oracle/e19_t7.py`.
Run at full scale, sequentially, removing each target's containers before the next (Docker label `oo.lane=mysql`, or
the label the harness requires; never touch other lanes' containers):
1. `tools/oo-test/oo-test matrix --engine mysql --versions 5.7,8.0,8.4,9 --suites T0,T1,T8 --hook T0=tools/oo-test/t0.py --hook T1=tools/oo-test/t1.py`
2. T2: `python3 tools/oo-test/t2_p0.py --engine mysql --versions 5.7,8.0,8.4,9` (C1 under the mysql-binlog build and C5)
3. T3/T4: `tools/oo-test/oo-test t3t4 --engine mysql --versions <same> --disk-wait-minutes 30`
4. T7: `python3 tools/oo-test/t7.py --engine mysql --versions <same>` (full scale default; Oracle adds --worker-image)
5. `tools/oo-test/oo-test matrix --merge-only --merge-evidence <each run dir>` → report the merged `h-merged-*`
   directory path and every row's result. Retries of a failed suite are separate runs; keep both.
Final report: version × suite/variant table, merged directory path, failures with cause.
