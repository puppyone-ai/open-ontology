# Linux amd64 `oo` for the L1 clean-machine run (built under emulation).
# Only the binary is exported: docker build --output type=local,dest=DIR
FROM docker.io/library/rust@sha256:f31fa9eaac4e417505e10009786ec0c636558e02dff3fa662cc92c10894ba065 AS build
ARG OO_LANE=a4
LABEL oo.lane=$OO_LANE
# The rust image (buildpack-deps) already has the C toolchain oo-cli needs;
# large LLVM/cmake downloads are not required for this crate.
WORKDIR /src
COPY . .
ENV CARGO_TARGET_DIR=/build/target CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4
RUN cargo build --locked -p oo-cli --bin oo && mkdir /out && cp /build/target/debug/oo /out/oo && rm -rf /build/target
FROM scratch
COPY --from=build /out/oo /oo
