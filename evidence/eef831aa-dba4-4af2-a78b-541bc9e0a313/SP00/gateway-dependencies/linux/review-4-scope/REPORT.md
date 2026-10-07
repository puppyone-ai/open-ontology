# Review 4. Owned-path scope

Reviewed commit `773c461104729d6c289eae8640c7c2a28207c447` on parent `b5ee3f4c7abad6a78341ac5b06ee21a19b7bea6e`. 1150 paths. Every path is `Cargo.toml`, `execution/decisions/gateway-http-dependency-baseline.md`, or `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a313/`.

`git diff --name-only` from the parent to this commit lists nothing under `Cargo.lock`, `rust-toolchain.toml`, `supply-chain/`, `crates/`, `tools/`, or `benchmarks/`.

`Cargo.lock` blob is `2990f901ea98a7b465f462cce8742bc5c51c13aa` at the fixed archive `b91b67bee0f603725b2cb7cc9c284588a2699392`, at the parent, and at this commit.

a207 commit `672129aa` (`evidence: record the file parser worker Linux run`) is still a commit object and an ancestor of this commit. Connector crate manifests are absent from this commit.

`supply-chain/dependency-baseline.json` is absent from this commit. The decision filename contains the words "dependency-baseline" and is the owned decision record, not that JSON file.

Child locks were not part of this commit. This review does not set the baseline to approved.
