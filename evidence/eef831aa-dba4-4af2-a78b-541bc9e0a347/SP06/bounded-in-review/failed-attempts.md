# Failed attempts

The product archive was not edited. The first final container exited during the pin check, before tests. The second final run is the accepting run.

1. `linux/final/attempt-1`. Toolchain 1.94.0 matched. The mounted file hashes matched `expected-sha256.txt`, and the review harness line was in a different order. `inside.sh` hashed `review_bounded_in.rs` third. The expected file lists it last. `pins.exit` is 1. `docker-run.exit` is 1. `inside.sh` now hashes the review file last. The container was removed.

The accepting run is `linux/final` with `final.exit` 0.
