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
# oo-mysql-doctor uses native-tls: link a pinned static OpenSSL built against the
# same glibc 2.31 floor, never the build image's system libssl.
RUN case "$TARGETARCH" in arm64) arch=aarch64 ;; amd64) arch=x86_64 ;; *) exit 2 ;; esac && \
    curl --fail --location --retry 3 "https://github.com/openssl/openssl/releases/download/openssl-3.5.9/openssl-3.5.9.tar.gz" -o /tmp/openssl.tar.gz && \
    echo "603f5602e2eef00d77fbd429d34dcd5822bb301757a1bc9cdb24c670f1eb859a  /tmp/openssl.tar.gz" | sha256sum --check && \
    mkdir /tmp/openssl && tar -xzf /tmp/openssl.tar.gz -C /tmp/openssl --strip-components=1 && \
    printf '#!/bin/sh\nexec zig cc -target %s-linux-gnu.2.31 "$@"\n' "$arch" > /usr/local/bin/oo-openssl-cc && \
    chmod +x /usr/local/bin/oo-openssl-cc && cd /tmp/openssl && \
    CC=oo-openssl-cc AR="zig ar" RANLIB="zig ranlib" ./Configure "linux-$arch" \
      no-shared no-dso no-engine no-tests no-docs --prefix=/opt/openssl --libdir=lib && \
    make -j4 build_libs && make install_dev && \
    cp LICENSE.txt /opt/openssl/LICENSE.txt && \
    cd / && rm -rf /tmp/openssl /tmp/openssl.tar.gz /usr/local/bin/oo-openssl-cc
WORKDIR /src
COPY . .
RUN rustc --version | grep '^rustc 1.98.1 ' && \
    target="$(rustc -vV | sed -n 's/^host: //p')" && \
    cargo zigbuild --release --locked -p oo-cli --bin oo --target "$target.2.31" && \
    cargo zigbuild --release --locked --manifest-path tools/mcp-gateway/Cargo.toml --target "$target.2.31" && \
    OPENSSL_DIR=/opt/openssl OPENSSL_STATIC=1 OPENSSL_NO_PKG_CONFIG=1 \
      cargo zigbuild --release --locked --manifest-path deploy/lite/mysql-doctor/Cargo.toml --target "$target.2.31" && \
    mkdir /out && cp "target/$target/release/oo" /out/oo && \
    cp "tools/mcp-gateway/target/$target/release/mcp-gateway" /out/mcp-gateway && \
    cp "deploy/lite/mysql-doctor/target/$target/release/oo-mysql-doctor" /out/oo-mysql-doctor && \
    /out/oo --version && /out/oo doctor --help && \
    echo '{}' | /out/oo-mysql-doctor | grep -q '"MYSQL_DOCTOR_INPUT"' && \
    ! readelf -d /out/oo-mysql-doctor | grep -Eq 'NEEDED.*lib(ssl|crypto)' && \
    glibc="$(objdump -T /out/oo-mysql-doctor | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -1)" && \
    test "$(printf '%s\n2.31\n' "$glibc" | sort -V | tail -1)" = 2.31
FROM scratch
LABEL oo.lane=b
COPY --from=build /out/oo /oo
COPY --from=build /out/mcp-gateway /mcp-gateway
COPY --from=build /out/oo-mysql-doctor /oo-mysql-doctor
