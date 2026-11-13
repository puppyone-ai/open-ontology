## Resume: V3-B-lane

The previous run of this card stopped mid-task on a model relay 502, not on a defect. Its commits are on this branch; uncommitted changes in the worktree are its unfinished work (rule 5: review, keep what is correct, finish). Continue the card below from where it stopped; do not redo committed parts. Do not delete files with rm -f; use git or targeted edits.

## V3-B lane — profiles, delivery backend, P1 friction, then MCP preflight
Do these cards in order, committing each separately: `V3-B-01-deliver-profiles.md` (non-UI parts only), `V3-P1-friction.md`, `V3-B-02-mcp-preflight.md`.
Plan: specs/v0.3-quickstart-and-competitive-plan.md. Commit as the owner, no agent trailers, trailer `Plan: <card id>`.

Write scope: packages/cli/**, tools/mcp-gateway/**, crates/oo-gateway (readiness/answerable endpoints only), crates/oo-stack (secret_ref sources), and non-UI modules in crates/oo-cli (new `profile.rs`, `check.rs`, `mcp_config.rs`, args wiring). A separate TUI lane owns crates/oo-cli/src/admin/wizard.rs, review.rs and any tui/ module: expose plain functions it can call (`profile::save`, `check::readiness`, `mcp_config::render(agent, profile)`, `share_prompt::render`) and document their signatures in `execution/v0.3/contracts/cli-backend.md` (you may create that file). Never put secrets in MCP config or argv.
