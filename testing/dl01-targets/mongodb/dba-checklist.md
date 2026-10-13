# DBA checklist

Synthetic database `oo_synth` on the standalone container `oo-dl01w1-14-mongodb`. Users live in `admin`. Passwords are files at `/run/secrets/{dba,reader,audit}` inside the container, mode 600, and are not written here.

| Identity | Role | Scope |
|---|---|---|
| `oo_dl01w1_14_dba` | `userAdminAnyDatabase` on `admin`, then `readWrite` and `dbAdmin` on `oo_synth` | Creates the role, the reader, the audit user, the index, and the 100-document load |
| `oo_dl01w1_14_reader` | built-in `read` on `oo_synth` | Catalog, batch cursor, cancel, one explicit change-stream attempt |
| `oo_dl01w1_14_audit` | custom `ooProfileRead` (`find` on `oo_synth.system.profile` only) | Reads the profiler |

Localhost bootstrap can grant `createUser` and `grantRole` on `admin` only. `readWrite` and `dbAdmin` on `oo_synth` are granted after the DBA authenticates. In MongoDB 8.0.32, `profile` with `slowms` also requires a process-global privilege, so the DBA sets level 2 and leaves the server default `slowms` of 100.

Rollback, as the DBA, touches only this fixture:

```javascript
db.getSiblingDB("oo_synth").dropDatabase()
db.getSiblingDB("admin").dropUser("oo_dl01w1_14_reader")
db.getSiblingDB("admin").dropUser("oo_dl01w1_14_audit")
db.getSiblingDB("admin").dropUser("oo_dl01w1_14_dba")
```

`ooProfileRead` is dropped with `oo_synth`. Host teardown is `scripts/cleanup.sh`, which removes this prefix only.
