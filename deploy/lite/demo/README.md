# Local OO-Logistics demo assets and runtime

The installed CLI deploys a curated, no-model demo with ten public synthetic
OO-Logistics tables. It uses the regular lite provisioning and retained
publication gates, installs the shipped hash-confirmed analyst Pack, enrolls
profile `demo`, and checks authenticated Gateway readiness.

```sh
oo quickstart --yes --json       # also: oo init --demo --yes --json
oo try --json
oo mcp-config --agent claude --profile demo
oo quickstart --down --json
```

Use a fresh working directory and HOME/profile namespace. The default state is
`evidence/oo-demo`; a custom `--state` must also be supplied to teardown.
Existing state and profile names are never overwritten. Teardown removes only
the deployment's verified `oo.lane=v03` containers, volumes and network;
private diagnostic state remains on disk. No image pruning is performed.

The synthetic source is a separate database on the control PostgreSQL server,
so the deployment uses two database containers, not three. The source reader
has SELECT-only data privileges. Replica-side region RLS is installed before
publication; the Gateway cannot query the source or access replica base tables.
Model egress and scheduled synchronization are not started for this demo.

The host adapter currently requires Python 3 in addition to Docker/Compose
and Node. This is local warm-image acceptance, not proof of a public npm/GHCR
release, a cold download, or a clean-machine result on every platform.

## Standalone fixture (not a published Gateway)

The older developer fixture below remains useful for testing seed/role SQL.
Its `fixture_ready` result is deliberately not quickstart acceptance.

### Local developer run

Use a repository-built lite image **content digest** already present in Docker.
GHCR is private; the public pull step is pending publication. This lane neither
pulls a mutable runtime tag nor pushes an image.

```sh
mkdir -p evidence
python3 deploy/lite/demo_runtime.py up-fixture \
  --state evidence/oo-demo --image sha256:YOUR_LOCAL_LITE_IMAGE_ID
```

Replace the placeholder with the 64-hex-digit local image ID. The separate
`deploy/lite/compose.demo.yaml` has a `demo` profile; the backend selects only
`demo-db`. It does not start the normal lite foundations or model-egress.
PostgreSQL is digest-pinned and must already be installed. No host ports are
published. The Docker network is internal.

The image generates a two-day one-off CA and randomized admin/reader passwords
into the new private state directory. The separate fixture-admin connection
seeds the tables. The product source identity `oo_demo_reader` has only SELECT,
uses TLS verify-full, and cannot write even after resetting its read-only
transaction default. The source is explicitly `local_fixture`; production TLS
defaults are unchanged.

`fixture_ready` means exactly that: **not published, no Gateway configured,
no enrolled CLI profile, no successful Agent answer**. The backend refuses
reuse of an existing state directory rather than reseeding or rotating secrets.
After a failure, run cleanup and use a new state directory.

```sh
oo quickstart --down --state evidence/oo-demo --json
# Before rebuilding the native CLI, the equivalent backend is:
python3 deploy/lite/demo_runtime.py down --state evidence/oo-demo
```

Teardown first checks every resource's project, `oo.lane=m` and unique `oo.demo`
labels, and the private owner record's state-directory binding. It deletes only
that set's containers, volumes and network, not images. Repeated down is safe.
Private credentials and diagnostic files are retained locally, not committed;
the next demo uses a fresh directory. Never use `docker system prune`.

## Policy and gold

Both NOLOGIN roles have one row filter on `demo.orders` and a column deny on
`demo.customers.phone_synthetic`. Analyst sees region 1; ops sees region 2.
The all-table source reader is a different identity used for capture.
`roles.yaml` describes the same intended Gateway policy. The standalone fixture tests prove source-fixture enforcement only. Quickstart
separately applies and exercises the region policy on the serving replica.

| Question | Analyst | Ops |
| --- | ---: | ---: |
| Visible order count | 60 | 60 |
| Exact visible amount, synthetic currency units | 3517.5000 | 3757.5000 |
| January calendar dates | 31 | 31 |
| Customer phone column | denied | denied |

`gold.json` fixes these SQL statements and exact results. The native `oo try`
backend uses the normal Gateway client and a local HTTPS endpoint; it never
queries PostgreSQL directly or substitutes expected rows. The npm wrapper
supplies the active profile's connection flags. Without a configured profile:

```sh
oo try --role analyst --json --gateway https://localhost:8443 \
  --transport production --auth external --token-file PRIVATE_DEMO_TOKEN_FILE \
  --ca-file PRIVATE_DEMO_CA_FILE --client-pem PRIVATE_DEMO_CLIENT_PEM
```

On a deployed local demo, `oo try` uses the enrolled profile automatically.
It runs all three fixed gold queries plus the denied query. Its demo-only
collector reads the actual Gateway WAL records, persists them in the control
DB's `oo_demo.audit_event`, and reads them back before reporting an audit ID.
A missing refusal record fails acceptance; a query ID alone is not proof.
The published generation comes from the real publication receipt, matched to
the authenticated Gateway freshness response. Source time remains unmeasured;
this is not a source-real-time or cross-table consistency claim.
For other local Gateway profiles without demo state, absent audit/generation
fields remain null and audit collection is explicitly not verified.

## Curated graph and Packs

`curate.py` invokes the real `selfbuild-budget` graph, registry, review and
reviewed-community compiler operations. No model transport is used. The
`public-demo-curator` reviewer is a fixture declaration, not a fabricated human
sign-off. It applies only to these shipped synthetic assets, never user data.

```sh
cargo build --locked -p oo-builder --bin selfbuild-budget
python3 deploy/lite/demo/curate.py --compiler target/debug/selfbuild-budget
python3 deploy/lite/demo_runtime.py verify-assets
```

`manifest.json` hashes the exact bytes of both reviewed inputs, both compiler
Packs, seed SQL, gold, adjudication and both confirmation envelopes. The role Packs omit the denied column
and all sample values. Compilation is not authentication or publication.

`adjudication.json` follows `oo.adjudication/v1`. `input_sha256` hashes the
exact `reviewed-analyst.json` file bytes, including its final newline.
`summary_sha256` hashes UTF-8 compact sorted-key JSON of
`{"graph": graph, "decisions": {item_id: decision}}` (`ensure_ascii=False`, no trailing newline).
The top-level `discovery` object (here `complete`, every declared edge checked)
is not hashed; the builder re-derives it from the input's draft edges.
Every synthetic key, declared relation and code coverage is checked by the
live fixture runner. The sensitive-column decision is explicitly declared,
not represented as a model prediction or a phone-format measurement.
The shipped `confirmed-*.json` envelopes bind the fixture confirmation to the
summary hash. The real compiler replays them and the publisher recomputes the
Pack before activation. This does not bypass confirmation for user databases.

## Remaining gates

- The delivered profile is analyst; the separately curated ops Pack is not
  enrolled by quickstart.
- `--rebuild`, live model-backed own-database onboarding and live A/B measurement
  require separate model configuration and acceptance. No model key is read by
  quickstart, and the replay A/B fixture is not a live model result.
- Public distribution, cold-image timing, other platforms and representative
  p50/p95 measurements remain separate gates.
- See `tools/oo-test/test_quickstart_live.py` for installed-package QS probes.
  Raw failures and successes are kept under ignored `evidence/` run directories.
