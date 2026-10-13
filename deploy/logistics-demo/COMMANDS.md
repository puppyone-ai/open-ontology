# PostgreSQL operator-development commands

The supported path is an authorized PostgreSQL source, a separate local
PostgreSQL target, an unreviewed draft Pack, and Pi through the native Gateway.
Other source engines are explicitly rejected. This is one operator's development
preview; it does not approve, classify, publish, or certify a production sandbox.

Start the complete path with one launcher command. The repository root is derived
from the script location; all overrides below are paths or public connection
metadata, never password or bearer bytes:

```bash
GPT_SOURCE_TEMPLATE=/private/source.json \
GPT_PRODUCT_TEMPLATE=/private/product.json \
GPT_SOURCE_PASSWORD_FILE=/private/source-reader.password \
GPT_SOURCE_CA_FILE=/private/source-ca.crt \
GPT_TARGET_ADMIN_PASSWORD_FILE=/private/target-admin.password \
GPT_TARGET_RUNTIME_PASSWORD_FILE=/private/target-runtime.password \
GPT_TARGET_READER_PASSWORD_FILE=/private/target-reader.password \
GPT_TARGET_CA_FILE=/private/target-ca.crt \
GPT_LLM_BEARER_FILE=/private/model-bearer \
GPT_NETWORK=approved-local-network \
GPT_SOURCE_CONTAINER='' \
bash deploy/logistics-demo/linux/closure/launch-run.sh
```

Use `source.example.json` and `config.example.json` as the exact schemas. In the
source file set `engine`, `host`, `port`, `database`, `user`, `server_name`, the CA
hash, `secret_ref`, approved `tables`, generation and UUID owner. Set the existing
local target host/port/database and its admin/runtime/reader usernames there too.
The product file names only that local target (`type: postgresql`, TLS `require`,
reader username, SecretRef and internal private-file paths), tables, model, Pi
and Pack paths. Target roles must already exist; the pipeline installs its local
schema and provisions the restricted development query role after sealing.

The frozen source budget is one connection, at most 1000 rows per batch,
statement timeout 30s, lock timeout 1s and snapshot deadline 300s. Copy is a
single `REPEATABLE READ READ ONLY` full snapshot. The target starts without the
selected imported data; partial generations are never admitted as ready.

The launcher runs `copy` and `prepare` in a parent container holding only copy
credentials. `prepare` acts only on a complete seal and grants the existing
local metadata and column permissions; it returns `ready: false` and does not
call the model. Afterward the launcher writes a credential-free replay sidecar,
detaches the explicitly named source container when applicable, starts the
runtime with only the target reader/model/public marker files, and runs Builder
and native Pack generation. Source hostnames distinct from the target are mapped
to a local sink in the runtime, preventing fake-IP DNS fallback. For an external
IP source, use the approved network boundary that excludes source access after
copy; this fixture's Docker disconnect is not an external firewall.

`GPT_SOURCE_CONTAINER` defaults to the explicitly owned demo source. Set it to
an empty string for an external source, or to a source container whose network
detach is authorized. Resource names, state/binary/Pi-kit volumes, private and
config directories, proof file, network and repository root have `GPT_*`
overrides in the launcher. Default fixture account names are examples, not a
requirement for a user-provided source or existing local target.

A failed model call returns nonzero and leaves no ready or published generation.
Resume that same sealed runtime with this command; validated batches are cached:

```text
minimal-product-loop run --config /var/lib/oo/config/product.json
```

`run` is the runtime resume/build entry after the launcher has isolated the
source. It is not the recommended single-process source-plus-model deployment.
Ask the natural question text, without SQL or an oracle:

```text
minimal-product-loop ask --config /var/lib/oo/config/product.json --question "<text>"
minimal-product-loop cancel --config /var/lib/oo/config/product.json
minimal-product-loop close --config /var/lib/oo/config/product.json
```

A released ask is refused. Close retains the persistent local database. The
native Gateway rejects writes and unsupported SQL. It currently admits count
star, sum of a numeric column, and min/max of an int8 column; aliases, subqueries,
natural joins, casts and other functions are rejected.

`gateway.query_pg` in the template is `/target/loop/debug/query-pg`, the rebuilt
native aggregate-aware binary, rather than the older kit binary. Its verified
SHA-256 is recorded in the closure evidence alongside the product binary.

There are two generation counters. `plan.generation` is the local mapping
counter; the existing Pack binding field `import_generation` names this counter.
`manifest.import_generation` is the target adoption counter. Query context names
both `mapping_generation` and `target_adopt_generation`, plus manifest/import
hashes and target-config hash. Compare each to its matching counter.
