# CLI parity matrix (D83, D85-D88; volume 25 section 7.1)

Status: **done** = in the app and tested; **partial** = in the app with a stated limit; **cli** = not in the app yet (use the equivalent command, which every screen shows); **other lane** = owned by another lane. Every app action shows its equivalent command (copyable).

| `oo` command / option group | App location | Status |
|---|---|---|
| `oo docs upload`, `oo source add fs` | Sources > Add source > Files / Folder | done (earlier lanes) |
| `oo source add s3\|oss\|cos\|r2` (+ credential options) | Sources > Add source > Cloud storage | done (earlier lanes) |
| `oo source add <kind>` for all 17 kinds: postgres mysql mariadb tidb oceanbase polardb opengauss kingbase sqlserver oracle db2 dm8 clickhouse doris starrocks mongodb opensearch (`--host --port --database --user --password-stdin --tls-ca-* --include --exclude`) | Sources > Add source > Database | done: the kind table (`db-kinds.mjs`) mirrors `source_db.rs` (stack engine, default port, TLS rule, scope unit: table, collection or index); live-tested from Add to Query on PostgreSQL 16 (MySQL 8.4 reaches the doctor, which needs a current oo-mysql-doctor beside oo); other kinds covered by the canned `oo` and the CLI's own tests |
| `oo source ls\|show\|rm\|test` | Sources list, database page (Remove, Test connection) | done |
| `oo source link ...` | not in the app | cli |
| `oo init --engine ... --password-file` (connection check) | Add database > Test connection | done (postgres, mysql, oracle) |
| `oo init --config`, `--catalog`, `--tables-*`, wizard, `--ssh` tunnel | first database writes the stack file; table scope is include/exclude | partial: no catalog browser, no SSH tunnel fields |
| `oo doctor` | Database page > Run doctor; Settings > Diagnostics | done |
| `oo up` / `oo build [--resume] [--review-file]` / `oo down`, `oo status` | Database page > Import data (a row per stage, log, freshness, the cause of a failed stage with its next step); the build review (below); `down`: channel only | partial: everything but `down` is in the UI |
| `oo build --review-file` (`oo.build-review/v1`, edge decisions as `oo.adjudication-override/v1`) | Database page > Review what was found: entities, relationships (declared, matched by name, found by values) with evidence, accept or reject each, one reason; then publish, Atlas source view, YAML files, Query | done (live-tested) |
| `oo review`, `oo ontology open\|export\|graph` | Ontology > Files (YAML per version, diff, export) | partial: version manifests from the engine and Gateway YAML; the Pack export is `oo ontology export` |
| `oo query`, `oo validate`, `oo tables`, `oo describe` | Ontology > Query (SQL, Tables) | done (`--expected-*`, `--client-pem`, `--connect-ip`, `--interface` not exposed) |
| `oo knowledge search\|cat` | Ontology > Query > Knowledge | done |
| `oo freshness [--summary]` | channel `cli-pipeline` (needs a Gateway) | partial: no screen yet beyond the database page status |
| `oo cancel`, `oo status <query_id>`, `oo operations` | not in the app | cli |
| `oo login`, `oo whoami`, `oo profile`, `oo use`, `oo token ...` | Query > Connect to a Gateway uses a token file | partial: login/profile/token issue via CLI |
| `oo check` | Settings > Diagnostics | done |
| `oo --version`, `oo docs runtime status`, `oo serve --status` | Settings > Diagnostics | done |
| `oo serve`, `oo docs runtime install` | Settings > Engine | done (earlier lanes) |
| one engine for `oo` and the app (state folder `OO_DOCS_STATE_DIR` or `~/.local/state/open-ontology/docs`, profiles in `OO_CONFIG_DIR` or `~/.config/open-ontology`) | Settings > Engine (shared state, active profile, "Use the CLI's data") | done (lane AH): an `oo serve` started by the CLI is attached; one the app starts is found by `oo`; an install with its own data keeps it until the person switches |
| live view of what the CLI does: `oo docs upload`, `oo source add fs\|s3\|<database>`, `oo init\|up\|build\|down`, `oo docs review decide`, ontology changes; `OO_AGENT_NAME=<name>` names an agent | Sources, Home (recent changes with an actor tag), file rows, Atlas, Query | done (lane AH): engine events with `actor` (docs-api 1.13) and `db.source_changed` / `db.pipeline_changed` announced by the CLI |
| `oo agent token`, `oo mcp-config`, `oo mcp` | Agents > Connect | done (earlier lanes) |
| `oo ontology search\|entity\|query`, entities, review | Ontology > Entities / Review / Conflicts | done (earlier lanes) |
| `oo context mount` and shares | Agents > Context packs | done (earlier lanes) |
| ontology atlas view | Ontology > Atlas | other lane (G) |
