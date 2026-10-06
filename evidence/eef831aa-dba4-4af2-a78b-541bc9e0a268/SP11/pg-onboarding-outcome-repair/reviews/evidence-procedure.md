# Review: evidence and procedure

Scope: this evidence directory and the launch scripts. The product archive `56710cc73cfe1663272ecaa503984e1f663428ad` was smoked before the repair was compiled. `linux/smoke/launch.sh` uses `git archive` of that commit. It does not mount the working tree.

Recipe origins and their hashes are in `origin/recipes.txt`. The copied hash tool is `tools/sha256_manifest.rs`, sha256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`.

Red runs mount an extracted archive plus the working-tree tests, and they compare `outcome.rs` to `source/blobs/outcome.rs` before start. Green mounts `tools/pg-onboarding` from the working tree and refuses a `Cargo.toml` or `Cargo.lock` diff against the fixed commit. The green bytes are commit `50d4e705a2bb4ba014919ef1e071da065cf8e9b7`.

Failed attempts keep their `console.log` and `test.log`. Later attempts use new directories. Builders are 6 GiB and 1 CPU, with `CapDrop=["ALL"]`, no privileged mode, no host network, and no pull. `linux/red-attempt-4/capacity.txt` and `linux/green/capacity.txt` record the gate. Other owners' builders were left running.

a261 evidence and resources were not modified. The live chain was not rerun. No root manifest or root lock edit is in the tool commit.

The inventory walker is the Rust program above. Hash oracles in the tests are the tool's own SHA-256. This review does not call the green exit an acceptance of the parent unit.
