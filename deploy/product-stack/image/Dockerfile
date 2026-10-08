# Precompiled service image. The base is the pinned Rust image already present locally.
# Build with --pull=false and --network=none. Do not push.
FROM docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
ARG BIN
COPY ${BIN} /usr/local/bin/service
USER 0:0
ENTRYPOINT ["/usr/local/bin/service"]
