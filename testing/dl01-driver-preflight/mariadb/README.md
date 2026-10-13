# MariaDB 11.4.13 driver preflight

Standalone Linux reader for the pinned MariaDB 11.4.13 fixture. It writes
Arrow IPC and a native catalog document. It does not accept MariaDB as a
source and it does not open PostgreSQL, Builder, or Pack.

The business read uses one `mysql_async` 0.36.1 connection. A second
connection, same reader account, is opened only by the cancel command to run
`KILL QUERY` against that first connection. The reader is not granted
`PROCESS` or `CONNECTION ADMIN`.

`testing/dl01-targets/mariadb` is the approved fixture input and is not
modified. Account names, labels, and secrets for this run are `w39_*` and
`oo-dl01w1-39`.
