# OceanBase C-S3S4: incomplete

Tested image: `oceanbase/oceanbase-ce:4.3.5.4-104000042025090916`, amd64.
Digest: `sha256:34701382e27e30ed5777e0d76e7e5abe23e186fc4c7078d7374f726840f44af4`.

Runs `c-s3s4-oceanbase-a` through `c-s3s4-oceanbase-h` are preserved under
ignored evidence. Initial pulls had TLS transport failures. Bootstrap needed
sshd, then tenant credentials and generated CA/server TLS were verified.
OB_VERSION returned 4.3.5.4 and compatibility mode MYSQL. Probe was reached;
no snapshot/C5 acceptance passed. Missing MySQL thread counters caused
source_busy; product now requires PROCESSLIST/PROCESS. A subsequent catalog
failure exposed the storage-label assumption. The final retry was interrupted
when host free disk fell to 166 MiB and Docker stopped responding.

Command:

```sh
python3 testing/engines/mariadb/run.py --family oceanbase --run-id c-s3s4-oceanbase-h
```

Cleanup was requested twice for the owned container
`oo-c-family-101c4126c0c1`; Docker returned API 500, then
`Docker Desktop is unable to start`. No service restart, unrelated deletion or
customer access was attempted. Its final stopped/removed state is unverified.
When Docker is restored, verify its `oo.lane=c` and
`oo.run_id=c-s3s4-oceanbase-h` labels before removing that exact container only.
All earlier owned fixtures were removed by their runners.

The runner now requires 16 GiB free disk before OceanBase, enforces lane
container limits, bounds Docker calls, and records cleanup failures without
losing the result. Do not retry until Docker is healthy and space is available.
