## Card REL-rehearsal — S6-B-01 release rehearsal (everything except publishing)

TODO: S6-B-01. Worktree branch `v0.2/rel` at `8b6c4fc2f23548297a22870755948751b6241a37`. Do NOT commit or edit tracked files (a report-only lane).
Never push any image or package to any public registry, never run `npm publish`, never `docker login`.
All outputs go under `evidence/rel-rehearsal/` (git-ignored). Read first: `.github/workflows/lite-images.yml`,
`.github/workflows/cli-binaries.yml`, `deploy/lite/docker-bake.hcl`, `ci/build-cli-linux.sh`, `ci/test-cli-linux.sh`,
`packages/cli/scripts/stage.mjs`, `packages/cli/scripts/licenses.py`. Reproduce what those workflows do, locally:

1. Lite runtime image, both architectures, exactly as `lite-images.yml` builds it (same bake file/targets), exported as an
   OCI image layout (`docker buildx build/bake ... --output type=oci,dest=...`; use a dedicated buildx builder with the
   docker-container driver, removed at the end). Record the image INDEX digest and both platform manifest digests.
2. Start a throwaway registry container (`registry:2`, label `oo.lane=rel`, port 127.0.0.1:5055) and copy the OCI layout
   into it preserving digests (`skopeo copy --all --preserve-digests oci:... docker://127.0.0.1:5055/oo/lite` via a
   skopeo container, or `crane push`/`oras` — whichever keeps the index digest byte-identical; prove it with
   `docker buildx imagetools inspect`). Pin: `127.0.0.1:5055/oo/lite@sha256:<index>`.
3. CLI binaries for all four ooPlatforms: darwin-arm64 (native, `MACOSX_DEPLOYMENT_TARGET=11.0`), darwin-x64
   (`rustup target add x86_64-apple-darwin` then cross-build), linux-x64 and linux-arm64 via `ci/build-cli-linux.sh`
   then `ci/test-cli-linux.sh`. Each with the `oo-lite-image` sidecar written by `stage.mjs`'s `checkImage`.
4. `licenses.py --output` notices, `stage.mjs` with all four `--binary` args (NOT --partial), `node --test
   packages/cli/scripts/stage.test.mjs packages/cli/test/*.test.mjs`, `npm pack --ignore-scripts --json`. Record
   tarball sha256/integrity/size and the file list.
5. Empty-directory install and every command, using `/opt/homebrew/bin/npm` (the zsh npm function is broken):
   - macOS arm64: fresh HOME and prefix, `npm install -g <tarball>`; run `oo --version`, `oo --help`, and `--help`
     for every subcommand listed by `oo --help` (exit 0 each); `oo doctor` offline behaviour; then a lite `oo up`
     against the local registry pin with a fresh deployment dir (label/lane `rel`), `oo status` healthy, `oo down`.
   - Linux amd64 and arm64: same install and help sweep inside `node:22-bookworm` containers (--platform), plus
     `oo up` on linux-arm64 only if the host daemon makes it practical (amd64 is emulated: note it, D1).
   - darwin-x64: `arch -x86_64 <bin>/oo --version` and `--help` under Rosetta.
   - Confirm `oo up` pulls by the pinned digest (`docker image inspect` RepoDigests) and the running containers use it.
6. Clean up: remove the registry container, the buildx builder, every container/volume/network labelled `oo.lane=rel`,
   and Cargo targets. Keep `evidence/rel-rehearsal/` (tarball, OCI layout digest list, logs, JSON summary).
Final report: digests, tarball identity, a table platform × step (pass/fail/skip with reason), exact commands, failures
with cause. If a product defect blocks a step, stop that step and report it; do not fix tracked files.
