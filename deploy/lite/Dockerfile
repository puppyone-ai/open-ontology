# Index digests contain both linux/amd64 and linux/arm64. Build one at a time.
FROM docker.io/library/rust@sha256:365468470075493dc4583f47387001854321c5a8583ea9604b297e67f01c5a4f AS build
ARG OO_LANE=m
LABEL oo.lane=$OO_LANE
RUN apt-get update && apt-get install -y --no-install-recommends cmake pkg-config libssl-dev clang libclang-dev protobuf-compiler && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
# Optimized build that keeps runtime checks: dev builds were too slow on small
# CPUs (Gateway catalog warm-up and queries; product lessons 2026-11-08).
ENV PROTOC=/usr/bin/protoc CARGO_TARGET_DIR=/build/target CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 \
    CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true
RUN cargo build --release --locked -p oo-cli -p oo-pipeline -p oo-gateway -p oo-builder --bins && \
    cargo build --release --locked --manifest-path deploy/lite/mysql-doctor/Cargo.toml && \
    cargo build --release --locked --manifest-path tools/oo-load/Cargo.toml --bin oo-load && \
    cargo build --release --locked --manifest-path tools/pg-builder/Cargo.toml --bin pg-builder && \
    cargo build --release --locked --manifest-path tools/release-coordinator/Cargo.toml --bin release-coordinator && \
    cargo build --release --locked --manifest-path tools/job-worker/Cargo.toml --bin job-worker && \
    cargo test --release --locked -p oo-pipeline --test native_library_publication --no-run && \
    mkdir /out && cp /build/target/release/oo /build/target/release/oo-pipeline \
    /build/target/release/oo-builder \
    /build/target/release/oo-mysql-doctor /build/target/release/pg-builder \
    /build/target/release/job-worker /build/target/release/oo-gateway /build/target/release/issue-token \
    /build/target/release/oo-load /build/target/release/selfbuild-budget \
    /build/target/release/release-coordinator /build/target/release/install-retained-target /out/ && \
    find /build/target/release/deps -maxdepth 1 -type f -executable \
      -name 'native_library_publication-*' -exec cp {} /out/oo-fixture-migrate \; && \
    test -x /out/oo-fixture-migrate && rm -rf /build/target
FROM docker.io/library/python@sha256:281d39ff02e3b389401586134ff9e6572954687822a58055ec07a60ded4e0b7f
ARG OO_LANE=m
LABEL oo.lane=$OO_LANE
RUN apt-get update && apt-get install -y --no-install-recommends libssl3 openssl ca-certificates libaio1 postgresql-client && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/ /usr/local/bin/
COPY tools/model-egress/model_egress.py /opt/oo/model_egress.py
COPY deploy/lite/draft.py /opt/oo/draft.py
COPY tools/pg-builder/scripts/ /opt/oo/pg-builder/
COPY deploy/lite/scheduler.py /opt/oo/scheduler.py
ENV PYTHONDONTWRITEBYTECODE=1
WORKDIR /work
ENTRYPOINT ["oo"]
