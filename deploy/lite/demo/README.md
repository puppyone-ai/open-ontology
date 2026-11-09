# Local OO-Logistics demo assets and runtime

This is the V3-M-01 data/runtime slice, not a completed quickstart. It ships a
10-table, 20,522-row public synthetic subset adapted from
`testing/engines/seed-common/generate.py`, two compiled role Packs and a graph.
No model endpoint, API key, customer database or existing token is accessed.

## Local developer run

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
`roles.yaml` describes the same intended Gateway policy. The live fixture tests
prove PostgreSQL enforcement only; they do not install that policy in Gateway.

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

Run this only after the coordinator integrates publication and enrollment.
The result contains SQL, rows, freshness and query receipts. Current query
receipts lack `audit_id` and `published_generation`: these stay null rather
than being invented from `query_id`. A matching column denial is not proof
of a persisted audit record. Wrong gold values, truncation, authentication
failure and unrelated SQL errors fail the demonstration.

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
Packs, seed SQL, gold and adjudication. The role Packs omit the denied column
and all sample values. Compilation is not authentication or publication.

`adjudication.json` follows `oo.adjudication/v1`. `input_sha256` hashes the
exact `reviewed-analyst.json` file bytes, including its final newline.
`summary_sha256` hashes UTF-8 compact sorted-key JSON of
`{"graph": graph, "items": items}` (`ensure_ascii=False`, no trailing newline).
Every synthetic key, declared relation and code coverage is checked by the
live fixture runner. The sensitive-column decision is explicitly declared,
not represented as a model prediction or a phone-format measurement.
The TUI may display this administrative graph without a model. It must bind
confirmation to the agreed hash; this lane does not supply a publish-confirm
record or alter the compiler's publication gate.

## Integration gates

- `oo quickstart` delegates to `oo init --demo`; the TUI lane owns screens.
  On this lane's base, `init --demo` is not implemented, so startup fails closed.
- Installing the source into Agent DB, activating the reviewed publication,
  applying both roles, issuing/enrolling local profiles and the final readiness
  check remain to be integrated. No scheduled sync is armed here.
- `--rebuild` remains pending the model-backed init path; no model call was made.
- QS-01 local and QS-02 full no-model startup are not passed. QS-03 requires
  a real Gateway deny and its durable audit record; PG denial is insufficient.
- QS-05 has local lifecycle evidence. Cross-platform clean installs, public
  distribution, install-to-first-answer p50/p95 and QS-04 belong to later gates.

Raw evidence is under `evidence/v3-m-20261109*/`; see `EVIDENCE.md` for the
committed summary. Failed startup attempts remain preserved.
