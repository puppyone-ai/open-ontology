# CLI parity matrix (D83, D85-D88; volume 25 section 7.1)

Status: **done** = in the app and tested; **partial** = in the app with a stated limit; **cli** = not in the app yet (use the equivalent command, which every screen shows); **other lane** = owned by another lane. Every app action shows its equivalent command (copyable).

| `oo` command / option group | App location | Status |
|---|---|---|
| `oo docs upload`, `oo source add fs` | Sources > Add source > Files / Folder | done (earlier lanes) |
| `oo source add s3\|oss\|cos\|r2` (+ credential options) | Sources > Add source > Cloud storage | done (earlier lanes) |
| `oo source add postgres\|mysql\|oracle\|sqlserver\|db2\|kingbase\|dm8` (`--host --port --database --user --password-stdin --tls-ca-* --include --exclude`) | Sources > Add source > Database | done; live-tested on PostgreSQL 16 and MySQL 8.0; other kinds covered by the canned `oo` |
| `oo source add mariadb\|tidb\|oceanbase\|opengauss\|polardb\|doris\|starrocks\|clickhouse\|mongodb\|opensearch` | listed in the kind picker, disabled with a note | cli: the connectors exist in `oo-connectors`, but `oo source add` / `oo doctor` accept only the seven kinds above; needs a CLI change per kind |
| `oo source ls\|show\|rm\|test` | Sources list, database page (Remove, Test connection) | done |
| `oo source link ...` | not in the app | cli |
| `oo init --engine ... --password-file` (connection check) | Add database > Test connection | done (postgres, mysql, oracle) |
| `oo init --config`, `--catalog`, `--tables-*`, wizard, `--ssh` tunnel | first database writes the stack file; table scope is include/exclude | partial: no catalog browser, no SSH tunnel fields |
| `oo doctor` | Database page > Run doctor; Settings > Diagnostics | done |
| `oo up` / `oo build` / `oo down`, `oo status` | Database page > Import data (progress, log, freshness); `down`: channel only | partial: `up`+`build` in the UI, `down` and `build --resume/--review-file` via CLI |
| `oo review`, `oo ontology open\|export\|graph` | Ontology > Files (YAML per version, diff, export) | partial: version manifests from the engine and Gateway YAML; the Pack export is `oo ontology export` |
| `oo query`, `oo validate`, `oo tables`, `oo describe` | Ontology > Query (SQL, Tables) | done (`--expected-*`, `--client-pem`, `--connect-ip`, `--interface` not exposed) |
| `oo knowledge search\|cat` | Ontology > Query > Knowledge | done |
| `oo freshness [--summary]` | channel `cli-pipeline` (needs a Gateway) | partial: no screen yet beyond the database page status |
| `oo cancel`, `oo status <query_id>`, `oo operations` | not in the app | cli |
| `oo login`, `oo whoami`, `oo profile`, `oo use`, `oo token ...` | Query > Connect to a Gateway uses a token file | partial: login/profile/token issue via CLI |
| `oo check` | Settings > Diagnostics | done |
| `oo --version`, `oo docs runtime status`, `oo serve --status` | Settings > Diagnostics | done |
| `oo serve`, `oo docs runtime install` | Settings > Engine | done (earlier lanes) |
| `oo agent token`, `oo mcp-config`, `oo mcp` | Agents > Connect | done (earlier lanes) |
| `oo ontology search\|entity\|query`, entities, review | Ontology > Entities / Review / Conflicts | done (earlier lanes) |
| `oo context mount` and shares | Agents > Context packs | done (earlier lanes) |
| ontology atlas view | Ontology > Atlas | other lane (G) |
