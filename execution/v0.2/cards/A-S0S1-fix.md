## Card A-S0S1-fix — lane A: close S0-M-04 / S0-A-01 / S1-A-01 sign-off gaps

TODO: S0-M-04, S0-A-01, S1-A-01. Lane label: `a`. This follows your previous run (report: STATUS partial).
The coordinator applied your coordinator.patch as commit 76c60df1f (oo-cli depends on oo-stack; patch
file removed). Your uncommitted tools/pg-builder/src/{config.rs,http.rs} edits are back in the worktree.

Fix, each as its own commit:
1. Streaming end-of-stream: an upstream stream is complete when a chunk carries a non-null
   `finish_reason` (and usage when requested), with or without a trailing `[DONE]`. This is a known lesson
   from production (docs/product/03 §3.5: "流式响应以 finish 帧为准，不依赖 [DONE]"). Only a stream that ends
   with neither is STREAM_INCOMPLETE. Re-run the authorized live_check (few calls) and the replay tests.
2. Capability probe model matching: /v1/models may list the configured model only with a channel prefix
   (e.g. `cx/gpt-6.1-sol`, `cb/gpt-6.1-sol`) and different context windows per channel. Match exact id first,
   then ids whose last path segment equals the configured id; when several match, record all and use the
   smallest context window and smallest max output as the budget (conservative), plus the measured
   system-prompt overhead. Unknown → fall back to stack values or conservative defaults, recorded as such.
3. Secret store: canonicalize parent directories (macOS /var → /private/var, dotfile-managed ~/.config
   symlinks are legitimate); still refuse when the secrets directory itself or a secret file is a symlink,
   wrong owner, or wrong mode. Make `secret_files_fail_closed` pass on macOS temp dirs; add a test for a
   symlinked ancestor being accepted.
4. pg-builder: commit your config.rs/http.rs edits pointing pg-builder at model-egress. The 3 failing
   pg-builder tests fail identically on the baseline (missing historical evidence); state that in the
   commit body with the exact test names and the baseline comparison — do not change those tests.
5. YAML admission: cargo-audit and cargo-deny are now installed at ~/.cargo/bin. Pick the YAML crate,
   run `cargo audit` and `cargo deny check advisories licenses` (add a minimal deny.toml only if the repo
   has none — check first), fill a complete supply-chain audit record per supply-chain/policy.md §1, and
   switch oo-stack to accept YAML (JSON still accepted). If admission still fails a policy field, keep JSON
   and record exactly which field blocks it.
Exit: cargo test -p oo-stack -p oo-cli --locked all pass; model-egress tests pass; live probe records window,
max output and overhead; cargo check --workspace --locked.
Write scope: as card A-S0S1 (crates/oo-stack, crates/oo-cli admin + Cargo.toml, tools/model-egress,
tools/pg-builder/src/{http,config}.rs, supply-chain/ new records, Cargo.lock for the YAML crate — report).
