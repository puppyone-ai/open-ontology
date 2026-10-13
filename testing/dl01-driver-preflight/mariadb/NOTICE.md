# Notices

This preflight is not a source release and does not redistribute MariaDB.

The pinned server image OCI label `org.opencontainers.image.licenses` is
`GPL-2.0`. The image's Debian copyright files point at
`/usr/share/common-licenses`, which the MariaDB package file list does not
ship. That license body is unresolved here. The copyright file hashes from the
approved preparation inventory stay the comparison point; this run re-hashes
the fresh container and records a mismatch if one appears.

`mysql_async` 0.36.1 states Apache-2.0 OR MIT in its crate header. Arrow 60
crates are Apache-2.0. Those crate texts are taken from the locked crates.io
artifacts at build time, not from a new registry resolution.
