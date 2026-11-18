# ADR-S14-docker-free-runtime

| Field | Value |
|---|---|
| Status | Proposed |
| Date | 2026-11-18 |
| Card | SO-B-S14-01 (spec 10 11.3, decision D30) |
| Supersedes | none |
| Superseded by | none |

Review of a Docker-free runtime for the document engine, with a measured prototype. Proposed is not Accepted: the sidecar half and the sandbox replacement below are open.

## Background

D30 made Docker a requirement of the first version. The engine already has a host runtime (`oo serve --runtime host`, the L2 path): `oo-docs-engine` runs as a child process of `oo serve` and takes its database from `OO_DOCS_DATABASE_URL`. Three things still need containers today: PostgreSQL (`docs-db`), the parser sidecar (`doc-parser`, `ocr-parser`, `media-parser`) and the isolation of hostile documents from the host and the network (`internal: true` network, read-only root, dropped capabilities, proved by `deploy/docs/test_no_egress.py`).

## Options

| Part | Option | Verdict |
|---|---|---|
| PostgreSQL | A. Embedded distribution: a pinned PostgreSQL 17 server build (about 80 MB unpacked) shipped with the app, `initdb` into the state directory on first run, `pg_ctl` supervised by `oo serve`, loopback and trust or SCRAM on a private port | Chosen. The schema installer, runtime role and tests are unchanged: the L2 flow ran against it |
| PostgreSQL | B. SQLite or an in-process store | Rejected. The engine relies on `_oo_docs` roles, advisory locks, transactional job queues and row security |
| PostgreSQL | C. User-installed PostgreSQL | Rejected for the default; allowed as `OO_DOCS_DATABASE_URL` for power users |
| Sidecars | A. `uv`-managed virtual environments, one per sidecar, from the existing `requirements.lock` files, models fetched by `models.lock.json` | Chosen for Markdown, text, CSV, Office and PDF. Needs a first-run download of wheels and models (several GB for Docling and OpenCV) |
| Sidecars | B. Keep the sidecar in Docker and only drop Docker for Markdown-only users | Fallback and the first shipping step: the engine already parses md, txt and csv without a sidecar |
| Sidecars | C. Frozen single-file sidecar binary | Rejected for now: the native stack (OpenCV, torch) does not freeze reliably across macOS versions |
| Merge engine and daemon | One process with the engine in-process | Deferred. Not needed for the goal and it widens the blast radius of a parser fault; a child process keeps the L2 supervision model |
| Isolation | macOS: `sandbox-exec` profile (deny network, deny file writes outside the scratch directory) around each sidecar process; Linux: user namespaces or bubblewrap | Open. `sandbox-exec` is deprecated by Apple but works on current macOS; this lane did not verify the profile, so no isolation claim is made. Until it is verified the Docker-free runtime must not parse untrusted Office or PDF files |

## Decision

Ship the Docker-free runtime in two steps. Step 1 (prototype, measured below): embedded PostgreSQL plus the host runtime, which serves Markdown, text and CSV Drives with no container. Step 2: `uv`-managed sidecars under a sandbox profile, only after the profile passes the no-egress and no-write-outside-scratch tests. The compose runtime stays the default for Drives that contain Office or PDF files.

## Prototype

`tools/oo-test/super_ontology_nodocker.py` runs the L2 flow without Docker: `initdb` and `pg_ctl` create and start a private cluster, the engine installs the schema, `oo serve --runtime host` starts, a 20 file Markdown Drive is registered, and the first BRIEF is compiled. The PostgreSQL binaries are the Homebrew 17.11 ones, standing in for the embedded distribution.

Measured on this Mac (arm64, debug builds of `oo` and `oo-docs-engine`, warm build caches, no network): see `testing/super-ontology/evidence/L8/nodocker.json` for the phase times. The figures start when the script starts and therefore exclude downloading the binaries and building the Rust code. They are not an install-to-first-BRIEF figure for a user: the unmeasured parts are the download of the app and of the PostgreSQL archive, and, for Office and PDF Drives, the first-run sidecar environment and model download, which is the dominant cost and is not covered here.

## Consequences

- The 10 minute target of the spec holds for Markdown, text and CSV Drives by a wide margin; it is not shown for Office and PDF Drives.
- `docs-db` memory and CPU limits (1 GiB, 1 CPU in compose) do not apply on the host; the engine's own limits do.
- The vector option (S13) needs pgvector in the embedded PostgreSQL build; without it the engine computes the same scores from block text, so the feature does not depend on it.
- Open: sandbox profile, sidecar environment build and its cold-start time, code signing and notarisation of the bundled PostgreSQL, and the upgrade path of the data directory across PostgreSQL minor releases.
