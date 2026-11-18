# Minimal pgvector image for docs-db vectors (B-S13-01, spec 90 section 3): the official postgres base plus
# pgvector 0.8.0 built from a pinned source tarball. The compiler never reaches the final image.
#
#   docker build --label oo.lane=super-ontology-c8 -t oo-docs-pgvector:dev deploy/docs/pgvector
#
# The base keeps its own entrypoint, so deploy/docs/postgres.sh (TLS and SCRAM wrapper) applies unchanged.
FROM postgres:17.11-bookworm@sha256:839fbb5a4942a7758dd93e18b42ea5649807491a0545b5eeeb8f0b8bb11857a5 AS build
ARG PGVECTOR_VERSION=0.8.0
ARG PGVECTOR_SHA256=867a2c328d4928a5a9d6f052cd3bc78c7d60228a9b914ad32aa3db88e9de27b0
ARG PGVECTOR_URL=https://github.com/pgvector/pgvector/archive/refs/tags/v0.8.0.tar.gz
ENV DEBIAN_FRONTEND=noninteractive
# Optional Debian mirror for hosts where deb.debian.org is slow: --build-arg APT_MIRROR=mirrors.aliyun.com
ARG APT_MIRROR=
RUN if [ -n "$APT_MIRROR" ]; then \
      sed -i "s#deb.debian.org#$APT_MIRROR#g; s#security.debian.org#$APT_MIRROR#g" /etc/apt/sources.list.d/debian.sources; fi \
 && apt-get update \
 && apt-get install -y --no-install-recommends build-essential ca-certificates curl postgresql-server-dev-17 \
 && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL "$PGVECTOR_URL" -o /tmp/pgvector.tar.gz \
 && echo "$PGVECTOR_SHA256  /tmp/pgvector.tar.gz" | sha256sum -c - \
 && mkdir /src && tar -xzf /tmp/pgvector.tar.gz -C /src --strip-components=1 \
 && cd /src && make OPTFLAGS="" -j4 && make install DESTDIR=/out \
 && cp /src/LICENSE /out/LICENSE.pgvector

FROM postgres:17.11-bookworm@sha256:839fbb5a4942a7758dd93e18b42ea5649807491a0545b5eeeb8f0b8bb11857a5
ARG OO_LANE=super-ontology-c8
LABEL oo.lane=$OO_LANE \
      org.opencontainers.image.title="oo-docs-pgvector" \
      org.opencontainers.image.description="postgres 17 with pgvector 0.8.0 (PostgreSQL License)"
COPY --from=build /out/usr/lib/postgresql/17/lib/vector.so /usr/lib/postgresql/17/lib/
COPY --from=build /out/usr/share/postgresql/17/extension/vector* /usr/share/postgresql/17/extension/
COPY --from=build /out/LICENSE.pgvector /usr/share/licenses/pgvector/LICENSE
