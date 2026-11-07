# Feature-isolated MySQL doctor

Build and install `oo-mysql-doctor` beside the `oo` executable:

```
CARGO_TARGET_DIR=target cargo build --manifest-path deploy/lite/mysql-doctor/Cargo.toml --locked
```

This lane-local package enables the already-locked connector `mysql` feature
without modifying the coordinator-owned CLI manifest or root lock. Its lock
contains no registry version absent from the root lock. It shares the exact
SourceEngine inspection implementation with PostgreSQL/Oracle doctor, including
`estimate_bytes`. The current MySQL adapter returns `None`: reports must retain
`null`/`unknown`, not infer bytes from rows.

Only source/table configuration with secret references crosses stdin; the helper
resolves private credentials itself. CLI resolves the helper beside its own
executable, never via PATH. Missing installation is `MYSQL_DOCTOR_INSTALL`, not
unsupported-engine success. MySQL requires a private CA reference and verifies
the configured hostname. No source writes or DBA grants are performed.

`images.json` pins the three card-requested patches missing from the shared
`oo-test` image lock. The test runner overrides only resolution for these patches
and still uses oo-test seeding, gold, resources and command recording. Every
created container/volume is labelled lane A and cleaned after label verification.
5.7.44 uses the shared amd64 pin. See the CLI's mysql-doctor-evidence.json.
