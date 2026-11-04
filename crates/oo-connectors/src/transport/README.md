# SSH source transport

`ssh::connect(source, secret_store)` is the stack-to-transport entry point.
It returns `None` for a direct source, or an `Arc<Tunnel>` for a pinned jump.
Use `PgSourceConfig::with_tunnel` / `MysqlEngine::with_tunnel` to retain the
guard and preserve the original database TLS identity. Never substitute
the loopback address for the database's certificate name.

The listener is IPv4 loopback-only, accepts at most four active forwarding
tasks, and forwards only to the configured database host and port. Each DB
connection gets a direct-tcpip channel. New connections retry transport
failures three times, with 250/500 ms backoff; authentication and pin failures
are terminal. Handshake/channel setup is bounded. Keepalive is 15 seconds,
with three unanswered probes allowed. Dropping the last guard aborts the
listener and channels; `Tunnel::shutdown` also waits for the listener task.
Interrupted streams are never replayed. The engine/loader remains responsible
for discarding the incomplete chunk and validating persisted resume receipts.

Authentication is exactly one `key.secret_ref` or `password.secret_ref`.
Private key files must be unencrypted (the secret store supplies file-mode
protection); agent, keyboard-interactive and passphrase prompts are not used.
The current crypto feature set supports Ed25519/ECDSA, not legacy RSA.
Driver/SSH error strings are not propagated with credential-bearing details.

## First-use integration hook (TUI owned by lane A)

After collecting the jump host, optional port (22), user and one authentication
reference, call `ssh::discover(host, port).await` if no pin was supplied, then
display the fingerprint through the existing raw-mode `Screen::prompt` and
require the exact answer `yes`. Store the confirmed value as
`source.ssh.host_key_sha256` using the existing stack writer before doctor/build.
For a normal terminal outside raw mode, `ssh::confirm_first_use` implements
this flow and refuses non-TTY input. Discovery never authenticates. Cancellation
does not create a pin. Subsequent connections never prompt over a mismatch.
No global known_hosts or SSH config is read or changed.

Suggested TUI text: "SSH jump host (optional); port [22]; user; key secret_ref
or password secret_ref (choose one). Verify this SHA256 host fingerprint out
of band, then type yes to pin it in stack.yaml. A changed key is refused.
Non-interactive runs require host_key_sha256."

The hook is supplied here; this lane deliberately does not edit `oo init` TUI
files. Until the TUI owner calls it, supply the pin explicitly in stack.yaml.
