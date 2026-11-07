## Card H-S4 — lane H: L1 clean-machine acceptance (S4 exit)

TODO: S4-M-02 (reassigned to lane H: lane M is on images/bundle). Exit = docs/product/04 §7 items 1–4 on macOS arm64
and Linux amd64 for MySQL 8.4.7, PostgreSQL 17.11 and Oracle 23.5. Rebase on v0.2/product-loops first. The whole chain
is integrated: `oo init` TUI, `oo build` (oo-load → Builder → owner review → publish-retained → Gateway → first
token → share prompt, `--resume`), `oo up/status` lite stack with scheduled cycles, SSH jump host and `oo up --target
ssh`. Do not edit product code; if acceptance finds a product defect, record it with evidence and the owning file, and
stop that case (the coordinator dispatches the fix). Harness code goes in `tools/oo-test/l1*.py`.
1. "Clean machine" on this host: macOS arm64 = a fresh empty HOME and working dir, the CLI installed only from the
   locally built `npm pack` of packages/cli plus the locally built native binary (no PATH leakage from the repo), Docker
   for the lite stack and the source DB. Linux amd64 = a `docker:dind` amd64 container (NOT systemd/privileged-init;
   never touch binfmt/sysctl) with the Linux amd64 CLI build inside, its own inner Docker for lite + source DB.
2. Item 1: drive `oo init` through its TTY (pexpect, recorded transcript) to the completion page's share prompt; then
   give a real-model Agent ONLY that prompt (model files as on card G-S5; bounded) and have it bind with the CLI and
   answer a fixed question set over the neutral customers/orders tables (use the gold questions that touch only
   scalar-key entities until lane K lands composite keys — say which). No manual file edits anywhere.
3. Item 2: same flow from a `stack.yaml` non-interactively; results identical.
4. Item 3: one SSH jump-host source and one `oo up --target ssh` deploy (reuse lane B/A2 harnesses).
5. Item 4: secret scan of argv (ps sampling), logs, stack.yaml, Packs and prompts (except `--raw-token`).
Record an evidence pack per platform × engine; failures honestly.
