# OceanBase CE owned fixture

C-S3b results and remaining loader blockers: [summary](../mariadb/C-S3b-SUMMARY.md).

```sh
python3 testing/engines/mariadb/run.py --family oceanbase --run-id <unique-id>
```

Pin: `oceanbase/oceanbase-ce:4.3.5.4-104000042025090916`, linux/amd64.
Resolved immutable digest is recorded before startup. Run this fixture alone;
it requests 6 GiB memory, 1 GiB data and 4 GiB log storage. TLS files and
bootstrap passwords are generated in the private ignored evidence directory.
The admin connection configures the disposable test tenant and TLS; the
product connection never bootstraps or changes source settings globally.

Product: `oo_connectors::mysql::oceanbase::OceanbaseEngine`, feature `mysql`.
Only CE 4.x MySQL mode is admitted. Product version is OB_VERSION(), not the
5.7 compatibility prefix. Synthetic binlog/OBLogProxy is a separate unsupported
integration. PROCESS is required to observe tenant-visible load; there is no
cross-tenant or cluster-wide pressure claim. Query timeout uses microseconds.
