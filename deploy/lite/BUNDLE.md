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
Host and daemon must use the bundle's architecture. Host prerequisites are not
installed by the bundle. Configuration and private secret-reference files are
operator-supplied; no credentials, customer configuration, review decisions or
database data enter the bundle.

The launcher loads images without pulling, then invokes the packaged CLI.
The lifecycle uses the verified local PostgreSQL ID and `--pull never --no-build`.
Normal non-bundle installs retain their prior missing-image pull behavior.
`down` uses the same launcher arguments and preserves database volumes.

**First up is still foundations-only.** The bundle does not invent an owner
review, install the missing control/job bindings, or claim Gateway publication.
`healthy:false` and the CLI's nonzero exit are preserved. Follow `RETAINED.md`
for the existing operator prerequisites; packaging is not automatic onboarding.

## Verification and cleanup

`test_bundle.py` exercises corruption, trust, archive paths/tags, architecture,
config identity and idempotent loading. `test_bundle_dind.py --bundle PATH
--manifest-sha256 SHA --run-id RUN` uses a fresh labelled `docker:dind` target
with no external network and a private cgroup namespace. It uses the standard
`dind` nesting wrapper, never systemd, binfmt or sysctl. It removes its outer
container/volume and runs at most two database containers. Its fixture-assisted
scheduled test is distinct from first-up acceptance and currently requires the
existing test fixture's migration bootstrap; a missing test binary/compiler is
a failure, not a passed scheduled cycle.

For a local-only signing proof, install the checksum-pinned cosign into an
ignored temporary directory with `bash ci/lite-cosign.sh DIRECTORY`, then run
`python3 ci/test-lite-signing.py --cosign DIRECTORY/cosign --manifest MANIFEST
--evidence evidence/RUN/signing`. It uses a temporary local key, disables
transparency uploads, verifies offline, and requires tampering to fail.
This does not test GitHub OIDC or registry signatures.

Retain raw logs, manifest hashes and build metadata in `evidence/RUN`. After
testing, delete the image tarballs and only the images built for that run.
Never prune the shared Docker daemon or delete pre-existing images.
