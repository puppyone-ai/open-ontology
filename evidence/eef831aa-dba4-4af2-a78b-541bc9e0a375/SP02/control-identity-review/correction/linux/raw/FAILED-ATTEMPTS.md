# Failed attempts

The proof container `oo-a375c-proof` exited 0. `run-proof.exit` is 0. There is no failed proof log.

One host launch failed before any builder existed. `pg/up.sh` calls `openssl x509 -ext subjectAltName`. The default `openssl` on this host was LibreSSL 3.3.6, which rejected `-ext`. The network `oo-a375c-eef831aa-net` had been created. No Postgres container and no Rust builder were created. The retry put `/opt/homebrew/opt/openssl@3/bin` first on `PATH` (OpenSSL 3.6.4) and ran the same `up.sh`. The script text was not changed.
