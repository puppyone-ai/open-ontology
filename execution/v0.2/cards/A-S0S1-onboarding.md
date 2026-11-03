## Card A-S0S1 — lane A: stack schema, admin CLI skeleton, model egress proxy

TODO: S0-M-04 (delegated, ledger E015), S0-A-01, S1-A-01. Lane label: `a`. Spec: REQ-ONB-01/02/05, AT-ONB-02/04. Decision D4.

Read: docs/product/04-onboarding-and-gateway-binding.md §2–4, docs/12-configuration-reference.md §9,
supply-chain/policy.md (mandatory for any new crate), crates/oo-cli (args.rs, load.rs),
packages/cli/bin/oo.mjs, tools/pg-builder/src/{http.rs,config.rs}, tools/source-onboarding/CONTRACT.md.

Deliver:
1. S0-M-04: new crate crates/oo-stack — serde types for `oo.stack/v1` exactly per docs/12 §9; validator that
   rejects inline secrets (any secret-looking value not given as `{secret_ref: name}`), bad secret_ref names,
   bad host_key_sha256, bad intervals, non-https URLs except loopback, unknown fields; secret store helper
   (`~/.config/open-ontology/secrets/<name>`, dir 0700, file 0600, refuse symlinks / wrong owner; reuse
   crates/oo-cli/src/load.rs checks). YAML: choose one maintained crate and add a complete audit record per
   supply-chain/policy.md §1 (published ≥7 days before adoption, license evidence, checksum, advisory scan;
   if a scanner is unavailable record that honestly). If no candidate can be admitted honestly, ship JSON
   now and record YAML as blocked. You may add crates/oo-stack to root Cargo.toml members and update
   Cargo.lock (report).
2. S0-A-01: admin subcommands in crates/oo-cli — `oo init|up|down|status|doctor|build|ontology|token|share-prompt`
   with `--help`, `--config stack.yaml`, `--yes`, `--json`; non-TTY fully non-interactive; errors never echo
   secrets; consumer commands unchanged. `oo init --config` validates and writes a normalized stack file;
   the interactive TUI is S2 (a clear "not yet" message is fine). Contract tests for argument errors.
3. S1-A-01: tools/model-egress — loopback-only listener; key read from a secret_ref file (never argv/env
   dumps/logs); forwards OpenAI-compatible `/v1/chat/completions` (stream and non-stream) to the configured
   base URL; optional Anthropic Messages translation; rate limiting; unified fail-fast codes for
   429 / usage_limit / cooldown; per-call usage metering to JSONL; capability probe (/v1/models when present
   + 1-token completion: context window, max output, JSON mode, proxy-injected system-prompt overhead =
   prompt_tokens of a minimal prompt). Point tools/pg-builder at it without breaking its loopback-only rule.
   Live test: endpoint `https://9r-admin.REDACTED/workflow/v1`, model `gpt-6.1-sol`, key in the 0600 file
   `~/.config/open-ontology/secrets/model-key-9r-sol`. Only a few real calls (probe, short completions, one
   stream). Never print, log, commit or copy the key. Offline record/replay tests for both dialects.

Write scope: crates/oo-stack/**, crates/oo-cli/src/admin/** + minimal wiring in crates/oo-cli/src/{main.rs,args.rs},
crates/oo-cli/tests/admin_*, tools/model-egress/**, tools/pg-builder/src/{http.rs,config.rs},
new files under supply-chain/, root Cargo.toml members + Cargo.lock (report). Prefixes `stack`, `cli`,
`model-egress`, `build(deps)`.
