# Zig supplies the glibc 2.31 sysroot; the host distribution is not the ABI floor.
FROM docker.io/library/rust@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS build
LABEL oo.lane=b
ARG TARGETARCH
RUN case "$TARGETARCH" in \
      arm64) arch=aarch64; sha=f7a654acc967864f7a050ddacfaa778c7504a0eca8d2b678839c21eea47c992b ;; \
      amd64) arch=x86_64; sha=24aeeec8af16c381934a6cd7d95c807a8cb2cf7df9fa40d359aa884195c4716c ;; \
      *) exit 2 ;; esac && \
    curl --fail --location --retry 3 "https://ziglang.org/download/0.14.1/zig-$arch-linux-0.14.1.tar.xz" -o /tmp/zig.tar.xz && \
    echo "$sha  /tmp/zig.tar.xz" | sha256sum --check && \
    mkdir /opt/zig && tar -xJf /tmp/zig.tar.xz -C /opt/zig --strip-components=1 && \
    rm /tmp/zig.tar.xz
ENV PATH=/opt/zig:$PATH CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
RUN cargo install cargo-zigbuild --version 0.23.2 --locked
WORKDIR /src
COPY . .
RUN rustc --version | grep '^rustc 1.98.1 ' && \
    target="$(rustc -vV | sed -n 's/^host: //p')" && \
    cargo zigbuild --release --locked -p oo-cli --bin oo --target "$target.2.31" && \
    cargo zigbuild --release --locked --manifest-path tools/mcp-gateway/Cargo.toml --target "$target.2.31" && \
    mkdir /out && cp "target/$target/release/oo" /out/oo && \
    cp "tools/mcp-gateway/target/$target/release/mcp-gateway" /out/mcp-gateway && \
    /out/oo --version && /out/oo doctor --help
FROM scratch
LABEL oo.lane=b
COPY --from=build /out/oo /oo
COPY --from=build /out/mcp-gateway /mcp-gateway
