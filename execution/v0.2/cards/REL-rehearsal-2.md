## Card REL-rehearsal-2 — S6-B-01 release rehearsal, second pass (fresh-machine oo up)

TODO: S6-B-01. Worktree branch `v0.2/rel` reset to `bac774f9ee8f3756eff2b7946b9ccfee2224aced`. Report-only lane: do NOT commit or edit tracked files; never
push to a public registry, never `npm publish`, never `docker login`. Outputs under `evidence/rel-rehearsal-2/`.
Pass 1 (`evidence/rel-rehearsal/`, read its REPORT.md and exact-commands.md first and reuse its method) found fresh
`oo up` stopping at LITE_IMAGES_MISSING; product commit b601f9d74 makes `oo up` pull digest-pinned images itself.

1. Rebuild only what changed: the oo CLI for darwin-arm64 and linux-arm64 and linux-x64 (same methods as pass 1); reuse
   pass 1's lite OCI layout (index sha256:a94f139a8cf62d74001ec2659f80bb20fe0b5447a446c519bfa4d9279fd1daa3) — the
   runtime image does not contain oo-cli; if `deploy/lite` or any crate in the image changed between pass 1's base and
   this commit (`git diff --stat <pass1-base> HEAD -- deploy crates tools/oo-load`), rebuild it instead and say so.
2. Throwaway registry (label oo.lane=rel, 127.0.0.1:5055), digest-preserving copy, stage all four platforms, npm pack.
3. Fresh macOS arm64 HOME and prefix, fresh Docker state for that image (remove any local copy of the pinned runtime and
   Postgres images first: `docker image rm` by the pinned references only): `npm install -g` the tarball; `oo init`
   non-interactively from a minimal stack.yaml against a throwaway PostgreSQL 17 source container (label oo.lane=rel,
   fixture data only), `oo up` (must pull by itself), `oo status`/health, `oo build` to a draft if the stack allows
   without a model key (otherwise stop after up and say why), `oo down`. Prove RepoDigests of the pulled runtime image
   equal the pin and the running containers use that image ID.
4. Same `oo up` fresh-pull proof inside a Linux arm64 `node:22-bookworm` container using the host Docker socket.
5. Note `oo help --help` exit 2 (pass 1) with the clap source of that behaviour; do not fix it.
6. Clean up everything labelled oo.lane=rel, the registry, builders, Cargo targets. Keep the evidence directory.
Final report: tarball identity, platform × step table, digest proofs, exact commands, failures with cause.
