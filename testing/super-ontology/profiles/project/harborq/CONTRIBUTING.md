# Contributing

Thanks for helping. Harbor Queue is maintained by a small team.

## Workflow

1. Fork the repository and create a branch with git checkout -b my-change.
2. Make one focused change and add a test for it.
3. Run cargo test --workspace and pnpm --dir web test.
4. Open a pull request that links the issue.

## Pull request rules

- Commit messages must use the imperative mood.
- Every pull request needs at least one reviewer from the Platform Team.
- Do not force-push to a branch that already has review comments.
- Update CHANGELOG.md for every user visible change.

## Where things live

The Rust core is in core/src/. Python tools are in tools/. The dashboard is in web/src/.
