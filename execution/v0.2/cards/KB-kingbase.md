## Card KB — lane KB: KingbaseES V9R1C10 (P2)

TODO: S5-E-03 (KingbaseES part; plan exit T0–T1). The owner downloaded the official image
`/Users/redux/Downloads/KingbaseES_V009R001C010B0004_aarch64_Docker.tar` (aarch64, runs natively here). Commit trailers:
`Plan:` only — no Co-Authored-By. Never copy the tar into the repo; `docker load` it and record its sha256 and the loaded
image id in testing/engines/kingbase/ (the developer license inside the image is what we test with; do not alter it).
KingbaseES is PostgreSQL-derived: first try the existing PostgreSQL SourceEngine against it (wire protocol, catalog,
`kingbase` default user/port 54321, `DB_MODE` pg or oracle). Where it differs, add a `kingbase` family with the smallest
dialect overrides (like MariaDB on the MySQL engine), never forking the PG engine. oo-load dispatch, doctor registration
(mirror the Db2 arm in crates/oo-cli/src/admin/doctor.rs), matrix target id.
Run with explicit verdict rows: T0 (product doctor), T1 (concurrent writer, compare after one C5 correction cycle), T8,
three C5 cycles (idle zero writes); C1 through logical decoding if the image supports it (else record why not).
Docker label `oo.lane=kb`; the image's documented run needs `--privileged … /usr/sbin/init` — allowed for this one
container only; never touch binfmt/sysctl; remove what you create.
