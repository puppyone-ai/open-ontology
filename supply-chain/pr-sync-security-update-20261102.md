# Product sync dependency security update

This change updates the workspace dependency graph without adding advisory
ignores, license exceptions, new trusted roots, or a minimum-age waiver.

- Tokio 1.53.1 -> 1.53.3 fixes the AsyncFd safety advisory.
- rustls 0.23.45 -> 0.23.46 and rustls-webpki 0.103.16 incorporate the
  applicable security fixes. Existing TLS providers, hostname verification,
  identity binding, and custom roots remain configured by the same callers.
- ctutils 0.4.4 and webpki-root-certs/webpki-roots 1.0.10 replace affected lock
  entries. The lock includes optional/target-specific packages even when the
  shipping Linux feature graph does not use them.
- mysql_async 0.37.1 replaces the vulnerable lru 0.14 dependency with lru 0.18.
  Its minimal-rust compression feature replaces minimal (native zlib).
  TLS stays native-tls-tls. The protocol compression support is retained;
  libz-sys and its environment-printing build script leave this graph.

The existing seven-day package-age gate still applies. The newer libz-sys fix
was not admitted early; the dependency was removed through the driver's
supported Rust backend instead. The exact graph is Cargo.lock. Independent
standalone tool workspaces have their own locks and are not certified by the
root workspace gate; those need coordinated dependency updates before release.

The deny configuration already named both supported GNU Linux architectures;
its validator now accepts exactly that existing pair, rejects omissions or
substitutions, and still rejects unknown configuration keys and weakened policy.
