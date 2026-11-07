# Lite images and offline bundles

`docker-bake.hcl` builds the existing shared runtime for both Linux architectures.
Gateway, worker/oo-load, Builder, scheduler, model-egress and operator bootstrap
select their entrypoints from that same image; they are not six independently
versioned builds. Base images and the SBOM generator are digest-pinned.

Local builds are unsigned and never push:

```sh
IMAGE=open-ontology/lite:local-arm64 docker buildx bake \
  -f deploy/lite/docker-bake.hcl --set runtime.platform=linux/arm64 --load \
  --metadata-file evidence/RUN/build.json
```

Use `linux/amd64` for the other architecture. Run serially and retain the
metadata. The CI workflow uses native runners (no binfmt installation), publishes
SBOM/provenance attestations, merges exact digests, and signs/verifies both
platform indexes and the merged index with keyless cosign. It runs only by
manual dispatch on the integration branch, behind `lite-release`. Configure that
GitHub environment's approval policy before enabling publication. No local build
or workflow definition is evidence that CI signing ran or that S5 exited.

## Create

The creator requires a Docker CLI/daemon supporting `image inspect --platform`
and `save --platform`. Images must already be local; creation never pulls.
Supply immutable runtime and PostgreSQL references, not tags:

```sh
python3 deploy/lite/bundle.py create --platform linux/arm64 \
  --runtime sha256:RUNTIME_DIGEST \
  --postgres docker.io/library/postgres@sha256:POSTGRES_PLATFORM_DIGEST \
  --output evidence/RUN/bundle-arm64
```

The output contains one-architecture, tagless image archives; the Linux CLI and
its companion executables copied from that runtime; Compose/scripts; a manifest
and SHA256SUMS. Creation refuses an existing output directory. A failed creation
leaves its partial directory for diagnosis; use a new directory on retry.
Transfer the printed manifest SHA-256 through a trusted channel separately.
Checksums alone are not publisher authentication.

The creator handles Docker's different OCI-index, platform-manifest and classic
config IDs. Installation verifies all files and archives before invoking the
shared `remote.py` save/load verifier. It rejects extra images, tagged archives,
symlinks, path escapes, wrong architecture and mismatched config/layer identity.
It never deletes existing images or volumes.

## Install

Use a trusted checkout's verifier before executing code from a received bundle:

```sh
python3 deploy/lite/bundle.py verify --bundle /media/bundle-arm64 \
  --manifest-sha256 TRUSTED_MANIFEST_SHA256
/media/bundle-arm64/oo up --bundle /media/bundle-arm64 \
  --manifest-sha256 TRUSTED_MANIFEST_SHA256 --config stack.json --json
/media/bundle-arm64/oo status --bundle /media/bundle-arm64 \
  --manifest-sha256 TRUSTED_MANIFEST_SHA256 --config stack.json --json
```

This `oo` is the bundle-local Python launcher, not a new Rust admin flag.
Lane A3 owns that parser. The target needs Python 3.11+, Docker with Compose,
OpenSSL and a glibc Linux userspace compatible with the runtime's Debian base.
The client and daemon normally use the bundle's architecture. For a Linux
amd64 client on an arm64 Docker daemon with working emulation, explicitly pass
`--emulated-client`; the launcher pins `DOCKER_DEFAULT_PLATFORM=linux/amd64`.
This does not install or change emulators. Host prerequisites are not
installed by the bundle. Configuration and private secret-reference files are
operator-supplied; no credentials, customer configuration, review decisions or
database data enter the bundle.

The launcher loads images without pulling, then invokes the packaged CLI.
The lifecycle uses the verified local PostgreSQL ID and `--pull never --no-build`.
Normal non-bundle installs retain their prior missing-image pull behavior.
`down` uses the same launcher arguments and preserves database volumes.

**First up provisions the reserved operator role and remains foundations-only.**
The packaged CLI installs the control token store, issues a deployment-local
operator token and writes its health-client references. It does not invent an owner
review, install control/job bindings, or claim Gateway publication.
`healthy:false` and the CLI's nonzero exit are preserved. Follow `RETAINED.md`
for the existing operator prerequisites; packaging is not automatic onboarding.

## Verification and cleanup

`test_bundle.py` exercises corruption, trust, archive paths/tags, architecture,
explicit client emulation, config identity and idempotent loading.
`test_bundle_dind.py --bundle PATH
--manifest-sha256 SHA --run-id RUN` uses a fresh labelled `docker:dind` target
with no external network and a private cgroup namespace. It uses the standard
`dind` nesting wrapper, never systemd, binfmt or sysctl. It removes its outer
container/volume and runs at most two database containers. For amd64 pass
`--host-daemon`: the emulated client mounts the host Docker socket and launches
explicit amd64 sibling containers. The evidence names the host daemon; this is
not a fresh or isolated amd64 daemon. The client joins only its owned fixture
network. Local TCP forwards connect the fixture's loopback endpoints to its
database and Gateway siblings; they do not terminate TLS or fabricate replies.
This avoids assuming Docker Desktop's published loopback ports are reachable
from another container. No host networking or emulator settings are changed.

Only a separately staged synthetic Python test harness and the bundle are
mounted, never the checkout, Cargo or host product binaries. The bundle includes
`oo-builder`, the compiled `oo-fixture-migrate` test helper, and the exact jobs,
token-store and publisher SQL used by the fixture. Builder Python modules come
from the bundled runtime image. `oo-fixture-migrate` runs the existing
`retained_gateway_install --ignored --exact` administrative test against the
fixture's private target config; it is **not a general production migration
CLI**, and is never invoked by ordinary `oo up`. It contains fixture-only jobs
scaffolding. The initial model transport and owner-review inputs remain
explicitly synthetic; scheduled workers and Gateway queries are real.

### A3 launcher contract

`bundle.launch(argv: list[str]) -> int` accepts `up|status|down`, required
`--bundle PATH --manifest-sha256 SHA`, optional `--emulated-client`, and forwards
the remaining CLI arguments to the bundled `bin/oo` with `--target local`.
It returns the CLI exit unchanged and rejects a caller-supplied `--target`.
The executable equivalent is `BUNDLE/oo` with the same arguments. A3 can invoke
this contract without changing bundle internals; no Rust admin or npm staging
files are modified here.

For a local-only signing proof, install the checksum-pinned cosign into an
ignored temporary directory with `bash ci/lite-cosign.sh DIRECTORY`, then run
`python3 ci/test-lite-signing.py --cosign DIRECTORY/cosign --manifest MANIFEST
--evidence evidence/RUN/signing`. It uses a temporary local key, disables
transparency uploads, verifies offline, and requires tampering to fail.
This does not test GitHub OIDC or registry signatures.

Retain raw logs, manifest hashes and build metadata in `evidence/RUN`. After
testing, delete the image tarballs and only the images built for that run.
Never prune the shared Docker daemon or delete pre-existing images.
