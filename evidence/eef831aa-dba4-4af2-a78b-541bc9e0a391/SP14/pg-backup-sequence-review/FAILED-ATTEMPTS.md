# Failed attempts

## identity-1

`failed-attempts/identity-1/`.

The first identity SQL concatenated `pg_depend.deptype`, which is `"char"`, and `psql` stopped before it printed the sequence name. The backup then ran against the source that still had schema `oo_a391_probe` owned by `oo_a356_admin`. The cut session exited with `permission denied for schema oo_a391_probe`. Backup exit 1. No manifest. The following restore exit 4 is manifest absence. It is not the identity withhold.

The retry cast `deptype::text`, dropped and recreated `shop.a391_ident` under `oo_a377_owner`, and transferred the probe schema to that owner. Before that backup, and after the success-path files were already saved, the source amount, party `p90000`, and key `k-9000` were returned to the sealed gold image so the shop copy could match. The restore databases were not reloaded. The retry result is in `identity/`.
