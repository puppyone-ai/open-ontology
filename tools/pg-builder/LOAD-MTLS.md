# Local oo-load publications

The `catalog` command selects the local adapter when any configured table is
in `oo_load_data`. That selection must contain only published views in that
schema. `_oo_load` storage generations are never datasets, and a mixed
selection fails closed. Ordinary source catalogs still use `catalog_bin`;
client-auth source catalogs outside this local adapter are not supported.

Example `postgres` configuration (file references, not PEM contents):

```json
{
  "host": "localhost",
  "port": 5432,
  "database": "agent",
  "username": "builder",
  "password_file": "evidence/run/.private/password",
  "sslrootcert": {"secret_ref": "evidence/run/.private/ca.pem"},
  "sslcert": {"secret_ref": "evidence/run/.private/client.pem"},
  "sslkey": {"secret_ref": "evidence/run/.private/client-key.pem"},
  "catalog_bin": "unused-for-publications",
  "tables": [{"schema": "oo_load_data", "table": "t_REPLACE_WITH_LOADER_VIEW"}]
}
```

These references are resolved local paths; the stack coordinator resolves named
SecretStore entries before constructing this configuration. Files must be
regular, non-symlink, single-link, 0600 files, at most 64 KiB. Client cert and key
are paired. `sslrootcert` replaces (cannot accompany) legacy `ca_file`. Legacy
`ca_file` remains supported. TLS is mandatory, with CA and hostname verification;
there is no insecure fallback. Profiling and comments use the same mTLS setup.

The reader needs USAGE on `oo_load_data` and `_oo_load`, SELECT on the selected
views, and SELECT on `_oo_load.current` and `_oo_load.publications`. It does not
need SELECT on physical data generations, nor any write permission. Grants are
administrator operations, not Builder operations.

Catalog capture uses a read-only repeatable-read transaction. Active publication
metadata is joined to the view's actual storage dependency. A detached view,
missing metadata privilege, or stale publication binding is refused. Primary
keys come from the active source chunk plan and retain composite member order;
they are source-observed logical keys, not enforced PostgreSQL view constraints.
The plan does not retain unique-index null semantics, so unique-index-only plans
are refused rather than emitting invented key semantics. The loader currently
retains no foreign-key catalog: this adapter cannot recover source relationships
that were never published. View/column comments are preserved as untrusted
evidence. Unavailable view statistics remain unavailable.

## Reproducible acceptance

```sh
CARGO_TARGET_DIR=target python3 tools/pg-builder/scripts/test_load_mtls.py \
  --run-id g-load-mtls-local
```

This uses tools/oo-test's pinned PostgreSQL 17 fixture and generator, loads
`public.regions` twice through the real oo-load binary, then changes only that owned
fixture to cert-only TCP authentication. It verifies catalog, profile, comments,
seal and cards, primary-key preservation, storage exclusion, missing cert/key,
CA and hostname rejection, inline-secret rejection and file permissions.
Two retained physical generations must produce exactly one dataset. This runner
does not call a model. A separate draft replay attempt was blocked by an existing
listener on the legacy Builder's fixed port 8317; it was not reused or stopped.
The required paid/live model call remains unverified.
All owned servers, containers and volumes are removed; raw evidence stays ignored.

The coordinator must wire oo-load's target view names and target connection into
the lite bridge, not re-use the source connection. `coordinator-load-mtls.patch`
is the minimal proposed bridge seam; its caller must supply `builder_postgres`
after a successful load. It has not been applied or acceptance-tested here.
