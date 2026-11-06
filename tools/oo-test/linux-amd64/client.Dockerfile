# A clean Linux amd64 machine for L1: Node/npm, python3 and the Docker CLI
# (static, with the compose plugin) driving the host daemon through its socket.
# No repository, Rust toolchain or Open Ontology helper is present.
FROM docker.io/library/docker@sha256:f649ef046008ca7f926a2571c32b0ac22e5c59eb61b959617f9acc2a4c638cf5 AS docker
# Debian 13 (glibc 2.41). The release CLI (ci/cli.Dockerfile, zigbuild) needs only
# glibc >= 2.30; a CLI built directly on the pinned Rust image needs >= 2.39.
FROM docker.io/library/node@sha256:54f84b4cf8432b21aaffd5d3ce8d41e12cea0d10d33c09d2f3c9879d49d6b446
ARG OO_LANE=a4
LABEL oo.lane=$OO_LANE
RUN apt-get -o Acquire::Retries=5 update && apt-get -o Acquire::Retries=5 install -y --no-install-recommends python3 openssl ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=docker /usr/local/bin/docker /usr/local/bin/docker
COPY --from=docker /usr/local/libexec/docker/cli-plugins/docker-compose /usr/local/libexec/docker/cli-plugins/docker-compose
