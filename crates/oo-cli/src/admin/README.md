# Admin skeleton integration

All native admin commands expose help/common flags; only `init --config` has
an implementation. Other operations fail with ADMIN_NOT_IMPLEMENTED. No input
is read from stdin, with or without a TTY; S2 owns the wizard. Init validates
first and atomically replaces the same config with normalized JSON, mode 0600.
A failed validation leaves original bytes untouched. Secrets are not resolved.

Consumer `status <query_id>` and its direct/help-alias output remain unchanged.
Bare status, status with only --json, or status with --config/--yes selects the
admin skeleton. Top-level help lists both surfaces.

The card allows args.rs but not the actual binary (src/bin/oo.rs), library,
run.rs or manifest. The allowed dispatch hook returns an existing Help outcome
for successful offline output; failure uses the existing sanitized formatter.
The schema source is shared with oo-stack via a path module, not copied.
`coordinator.patch` replaces that workaround with a proper crate dependency and
fixes npm's existing status profile injection. The patch is **not applied**;
the manifest and npm wrapper are outside this card. Regenerate Cargo.lock after
applying it (only the oo-cli -> oo-stack dependency edge should change).

Until the wrapper patch is applied, use the native binary for admin status:
a configured npm profile would otherwise inject consumer flags into it.
Other admin commands are not in the wrapper's connectedCommands set.

`cargo test -p oo-cli --locked` includes the new admin contract and unchanged
consumer tests. TUI, orchestration, token lifecycle and deployment are not
claimed by the skeleton.
