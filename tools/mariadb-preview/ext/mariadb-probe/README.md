The proof container mounts the committed probe tree
`testing/dl01-driver-preflight/mariadb/probe` over this directory.
That mount is read-only. Do not replace it with a temporary Cargo.toml.
`tools/mariadb-preview` defines `workspace.package` so the probe's
`edition.workspace = true` resolves. The inherited fields match the
probe workspace in `testing/dl01-driver-preflight/mariadb/Cargo.toml`.
From the archive root the check is:

`cargo test -p mariadb-preview --locked --manifest-path tools/mariadb-preview/Cargo.toml`

This placeholder keeps the mount point in the product tree.
