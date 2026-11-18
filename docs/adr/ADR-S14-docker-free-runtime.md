# ADR-S14-docker-free-runtime

| Field | Value |
|---|---|
| Status | Proposed |
| Date | 2026-11-18 |
| Card | SO-B-S14-01 (spec 10 11.3, decision D30) |
| Supersedes | none |
| Superseded by | none |

Review of a Docker-free runtime for the document engine, with a measured prototype. Proposed is not Accepted: the sidecar and the sandbox are now prototyped and measured on one Mac (below); signing, the model download and the OpenCV build are still open.

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
| Isolation | macOS: `sandbox-exec` profile (deny network, deny file writes outside the scratch directory) around each sidecar process; Linux: user namespaces or bubblewrap | Prototyped and self-tested (see Sidecar prototype). `sandbox-exec` is deprecated by Apple but works on this macOS. The profile blocks outbound network, writes outside the scratch directory, reads of the home directory and unix sockets outside scratch; it is not yet run against `deploy/docs/test_no_egress.py` or the hostile fixtures, so it is a measured prototype, not an accepted isolation |

## Decision

Ship the Docker-free runtime in two steps. Step 1 (prototype, measured below): embedded PostgreSQL plus the host runtime, which serves Markdown, text and CSV Drives with no container. Step 2: `uv`-managed sidecars under a sandbox profile, only after the profile passes the no-egress and no-write-outside-scratch tests. The compose runtime stays the default for Drives that contain Office or PDF files.

## Prototype

`tools/oo-test/super_ontology_nodocker.py` runs the L2 flow without Docker: `initdb` and `pg_ctl` create and start a private cluster, the engine installs the schema, `oo serve --runtime host` starts, a 20 file Markdown Drive is registered, and the first BRIEF is compiled. The PostgreSQL binaries are the Homebrew 17.11 ones, standing in for the embedded distribution.

Measured on this Mac (arm64, debug builds of `oo` and `oo-docs-engine`, warm build caches, no network): see `testing/super-ontology/evidence/L8/nodocker.json` for the phase times. The figures start when the script starts and therefore exclude downloading the binaries and building the Rust code. They are not an install-to-first-BRIEF figure for a user: the unmeasured parts are the download of the app and of the PostgreSQL archive, and, for Office and PDF Drives, the first-run sidecar environment and model download, which is the dominant cost and is not covered here.

## Sidecar prototype (added 2026-11-18)

`super_ontology_nodocker.py --sidecar [--sandbox] [--sandbox-selftest]` adds the Office and PDF parser. Everything below ran on this Mac (arm64, debug builds of `oo` and `oo-docs-engine`); the evidence is `testing/super-ontology/evidence/L8/nodocker-sidecar.json`.

- Environment: `uv venv --python 3.13` and `uv pip install --no-deps` of the 84 versions pinned in `tools/doc-parser/requirements.lock`. The lock's hashes are linux wheels, so on macOS the versions are installed without hashes and macOS wheels are resolved by version; a macOS lock is still to be generated. The package is 1.2 GB on disk, models 367 MB.
- Models: copied from `/opt/models` of the local `oo-docs-parser:dev` image. `huggingface.co` was not reachable from this host, so the real first-run model download was not tried or timed.
- Engine wiring: `oo serve --runtime host` now forwards `OO_DOCS_PARSER_URL` and `OO_DOCS_PARSER_TOKEN_FILE` from the operator environment to the engine (before this the host runtime could not use any sidecar). The sidecar runs as a child process on loopback.
- Drive: 20 Markdown files, one golden docx and one golden pdf. The first BRIEF waits until all 22 files are published.
- Sandbox: `tools/oo-test/nodocker_sidecar.sb` under `sandbox-exec`. Self-test from inside (the same probe unsandboxed in brackets): internet connect blocked (allowed), write to the home directory blocked (allowed), write to the scratch directory allowed (allowed), listing `~/.ssh` blocked (allowed), connect to the Docker unix socket blocked (allowed), loopback listen and connect allowed (allowed). The sidecar parses both Office files inside it.

| Measurement | Result |
|---|---|
| Markdown only, script start to first BRIEF | 5.5 s |
| Environment ready, uv cache warm, `--offline` (venv, 84 packages, models and app copied) | 2.9 s from script start |
| 22 files including docx and pdf, sidecar under `sandbox-exec`, uv cache warm, offline: script start to first BRIEF | 36.5 s |
| Of which Office parsing (two files published) | 33 s |
| Cold install of the same 84 packages, no uv cache, from a PyPI mirror | 16 s (default PyPI index: about 100 KB/s, torch alone did not finish in 35 minutes, so a cold first install is network bound) |

Honest reading: install to first BRIEF on this Mac is about 36 s with warm caches and about 52 s plus the model download with a fast mirror (16 s install plus the 36.5 s run). It excludes downloading the app, building the Rust code and fetching models from Hugging Face; the ten minute target is met where the network is fast and is not shown where it is not.

Findings that change the plan:

- The sidecar's default `forkserver` worker start died with signal 11 on the pdf on macOS arm64 once torch was imported; `OO_PARSER_START_METHOD=spawn` works and costs a torch import per request (about 8 s per parse in this run). A shipped macOS sidecar needs a warm worker pool or a fix of the fork path.
- The PyPI `opencv-python-headless` wheel bundles FFmpeg. D65 forbids that in a distributed build, so the prototype's table structure works only as a development stand-in; a shipped macOS build needs the source-built OpenCV without FFmpeg (10 to 25 minutes) or has to run PDFs without table structure.
- The sandbox self-test is not a security proof: it checks the denials listed above, not DNS tricks, the mach services Python can still reach or pipes to other processes. `sandbox-exec` is deprecated, so this is a stopgap until an App Sandbox helper or a launchd-hosted helper exists.

## Consequences

- The 10 minute target of the spec holds for Markdown, text and CSV Drives by a wide margin, and for Office and PDF Drives on a fast package mirror (about 52 s measured plus the model download); it is not shown for a slow network or the model download.
- `docs-db` memory and CPU limits (1 GiB, 1 CPU in compose) do not apply on the host; the engine's own limits do.
- The vector option (S13) needs pgvector in the embedded PostgreSQL build; without it the engine computes the same scores from block text, so the feature does not depend on it.
- Open: running the sandbox profile through `deploy/docs/test_no_egress.py` and the hostile fixtures, a macOS lock file with hashes, the Hugging Face model download and its time, the worker start method, OpenCV without FFmpeg, code signing and notarisation of the bundled PostgreSQL, and the upgrade path of the data directory across PostgreSQL minor releases.
