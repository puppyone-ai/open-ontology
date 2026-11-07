# tidb C-S3S4 evidence

Raw run: `c-s3s4-tidb-f` (git-ignored).

| Observed version | Image digest | Outcome |
|---|---|---|
| 8.0.11-TiDB-v7.5.0 | `pingcap/tidb@sha256:4a0f3070b27a5acf9734e16c725050ae95b4a07b58ac08693c596ce237045a1f` | connector snapshot/C5 passed |
| 8.0.11-TiDB-v8.5.0 | `pingcap/tidb@sha256:490861bd4a2fa643dbe76b77ffecafcad66e6a42774e580b82f4ec2429a80e57` | connector snapshot/C5 passed |

Command:

```sh
python3 testing/engines/mariadb/run.py --family tidb --run-id c-s3s4-tidb-f
```

Scope: five small tables, integer/string/composite keys, no-key and empty
tables, C5 baseline/no-change/mutation/no-change cycles. C5 replacements
compared to fresh connector reads. No Agent DB, Loader, publication, concurrent
writer T1 or end-to-end T0-T2 acceptance is claimed.

TiDB fixtures used unistore single-node with linux/amd64 emulation.
They do not establish TiKV/PD or distributed-cluster acceptance.

The checked-in final fixture adds independent whole-table typed comparisons,
additional decimal/unsigned/JSON/binary/TIME rows, missing-SELECT probes,
C1 image equality and DDL refusal. These enhanced live assertions compiled
but were not rerun after Docker became unavailable. Do not transfer the old
passing outcome to the strengthened fixture.

## Evidence SHA-256

- `c-s3s4-tidb-f/pingcap-tidb-v7.5.0/c5.json`: `3784d73c3e63364a51dc604ad7c0b5f5e7d3e3216a4046901f26abc9197cd036`
- `c-s3s4-tidb-f/pingcap-tidb-v7.5.0/image.json`: `2416554cdab2012c0ea24ca37d1666b1da9097a1b10ae5694e191d8cc430b2ce`
- `c-s3s4-tidb-f/pingcap-tidb-v7.5.0/plan-composite_rows.json`: `eb179712a743c883ad446fcc7b54a8009345f027da0ea77a59881213a8ac78a3`
- `c-s3s4-tidb-f/pingcap-tidb-v7.5.0/plan-empty_rows.json`: `9a48f0fc0c668d9d926c519d7f28040fd2ae02d81a2dfaa147d18cf7c5929197`
- `c-s3s4-tidb-f/pingcap-tidb-v7.5.0/plan-no_key.json`: `e7599ef50ef47e19598fb7a1ca2a8441c357154b18bcc1f4cfa2e1e452830aa3`
- `c-s3s4-tidb-f/pingcap-tidb-v7.5.0/plan-rows.json`: `0d87a4fe57e97d6bf002bb23f79b6f250d70b812f992db17afd523c830e97078`
- `c-s3s4-tidb-f/pingcap-tidb-v7.5.0/plan-text_rows.json`: `0312ab4c614e008d1ebd3606c4ba49411c2d2f70e1f41b754f1e23f951b8f538`
- `c-s3s4-tidb-f/pingcap-tidb-v7.5.0/probe.json`: `67f2056c3f42379a1d7ed61f9e3c8869293f705bb7d85d77b982cb72b0788ed3`
- `c-s3s4-tidb-f/pingcap-tidb-v7.5.0/snapshot.json`: `ab43c91f8c31666f25834990bdbeffbf961700550720fe89d4b3593ada72019f`
- `c-s3s4-tidb-f/pingcap-tidb-v8.5.0/c5.json`: `3784d73c3e63364a51dc604ad7c0b5f5e7d3e3216a4046901f26abc9197cd036`
- `c-s3s4-tidb-f/pingcap-tidb-v8.5.0/image.json`: `a7d063e67a4f73f9400a96bcd1f683562b30f90f130b7f2132a667cceecc0568`
- `c-s3s4-tidb-f/pingcap-tidb-v8.5.0/plan-composite_rows.json`: `eb179712a743c883ad446fcc7b54a8009345f027da0ea77a59881213a8ac78a3`
- `c-s3s4-tidb-f/pingcap-tidb-v8.5.0/plan-empty_rows.json`: `9a48f0fc0c668d9d926c519d7f28040fd2ae02d81a2dfaa147d18cf7c5929197`
- `c-s3s4-tidb-f/pingcap-tidb-v8.5.0/plan-no_key.json`: `e7599ef50ef47e19598fb7a1ca2a8441c357154b18bcc1f4cfa2e1e452830aa3`
- `c-s3s4-tidb-f/pingcap-tidb-v8.5.0/plan-rows.json`: `0d87a4fe57e97d6bf002bb23f79b6f250d70b812f992db17afd523c830e97078`
- `c-s3s4-tidb-f/pingcap-tidb-v8.5.0/plan-text_rows.json`: `0312ab4c614e008d1ebd3606c4ba49411c2d2f70e1f41b754f1e23f951b8f538`
- `c-s3s4-tidb-f/pingcap-tidb-v8.5.0/probe.json`: `af03b404c40e1a2665d1bde399e7cfe1edae69accaf994222e91a063906e6ac0`
- `c-s3s4-tidb-f/pingcap-tidb-v8.5.0/snapshot.json`: `ab43c91f8c31666f25834990bdbeffbf961700550720fe89d4b3593ada72019f`
- `c-s3s4-tidb-f/pingcap-tidb-v7.5.0/test.log`: `d41e323efd80a41121520017d370d5d14249211ec6d683fae506ef5b85db13c4`
- `c-s3s4-tidb-f/pingcap-tidb-v8.5.0/test.log`: `e7cf078fee2605fdfe35e483cc828e9bbba9ec4d6a6e8b8d5408f4b21b00f393`
