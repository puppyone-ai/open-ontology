# Linux/MSRV focused validation command contract

Coordinator reference for future frozen task prompts. This file does not certify a run. Each executor must record actual image digest, toolchain, architecture, immutable input inventory, argv, raw stdout/stderr and exits. First smoke happens before bulk edits, final focused verification follows implementation. Preserve both failed and replacement attempts.

Use a finite snapshot of the intended manifest/workspace inputs and a task-owned target directory. Set RUSTUP_TOOLCHAIN=1.94.0 inside the pinned Linux builder; record rustc -vV and cargo -V. Install the pinned clippy component explicitly if absent. Required development architecture is linux/arm64 here; full arm64/amd64 matrix is deferred to integration checkpoints unless a new native/platform defect requires a precise probe.

Root-workspace example, replace oo-builder only with the assigned package:

    cargo fetch --manifest-path Cargo.toml --locked
    cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder
    cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --all-targets -- -D warnings

If core contract changes or is an explicit prerequisite under test, add -p oo-core to test/Clippy. Never pass -p to cargo fetch: fetch resolves the specified manifest and its lock, it has no package-selection flag. Package selection belongs to test/build/Clippy. Fetch failure must be fixed before offline phases; do not declare an air-gap build because one fetch succeeded with network.

Standalone-workspace example:

    cargo fetch --manifest-path testing/spikes/ASSIGNED_SPIKE/Cargo.toml --locked
    cargo test --manifest-path testing/spikes/ASSIGNED_SPIKE/Cargo.toml --locked --offline
    cargo clippy --manifest-path testing/spikes/ASSIGNED_SPIKE/Cargo.toml --locked --offline --all-targets -- -D warnings

Cargo manifests and locks remain within assigned writer scope. Generating a new task-specific lock is a dependency change, not smoke verification of an existing lock: inspect exact official publish time, checksum, yanked state, MSRV and selected license obligations before claiming the new dependency accepted. Do not modify unrelated pins merely to get a compile.

Inspect real phase exits rather than relying on an outer wrapper. Integer exit zero is a valid success value and must not be replaced by a truthiness fallback such as value or 1. Missing exit records remain missing. Inspect Docker HostConfig.Tmpfs when verifying temporary mounts; it need not appear under Mounts. Do not rewrite earlier exit files after correcting bookkeeping.

Use task-owned labels/IDs, max one builder per task and max two builders globally. Observe active owned labels and defer when full; no global prune or foreign cleanup. Executable test shims must use an executable task target path rather than a noexec temporary mount. Git test identity/config must be private per command and never mutate real HOME/global Git configuration. Archive logs before deleting exact owned resources; cleanup failure is recorded separately from test failure.
