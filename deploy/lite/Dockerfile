# Index digests contain both linux/amd64 and linux/arm64. Build one at a time.
FROM docker.io/library/rust@sha256:365468470075493dc4583f47387001854321c5a8583ea9604b297e67f01c5a4f AS build
LABEL oo.lane=a
RUN apt-get update && apt-get install -y --no-install-recommends cmake pkg-config libssl-dev clang libclang-dev protobuf-compiler && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
ENV PROTOC=/usr/bin/protoc CARGO_TARGET_DIR=/build/target CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4
RUN cargo build --locked -p oo-cli -p oo-pipeline -p oo-gateway --bins && \
    cargo build --locked --manifest-path deploy/lite/mysql-doctor/Cargo.toml && \
    cargo build --locked --manifest-path tools/oo-load/Cargo.toml --bin oo-load && \
    cargo build --locked --manifest-path tools/pg-builder/Cargo.toml --bin pg-builder && \
    cargo build --locked --manifest-path tools/job-worker/Cargo.toml --bin job-worker && \
    mkdir /out && cp /build/target/debug/oo /build/target/debug/oo-pipeline \
    /build/target/debug/oo-mysql-doctor /build/target/debug/pg-builder \
    /build/target/debug/job-worker /build/target/debug/oo-gateway /build/target/debug/issue-token \
    /build/target/debug/oo-load /out/ && rm -rf /build/target
FROM docker.io/library/python@sha256:281d39ff02e3b389401586134ff9e6572954687822a58055ec07a60ded4e0b7f
LABEL oo.lane=a
RUN apt-get update && apt-get install -y --no-install-recommends libssl3 openssl ca-certificates libaio1 && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/ /usr/local/bin/
COPY tools/model-egress/model_egress.py /opt/oo/model_egress.py
COPY deploy/lite/draft.py /opt/oo/draft.py
ENV PYTHONDONTWRITEBYTECODE=1
WORKDIR /work
ENTRYPOINT ["oo"]
