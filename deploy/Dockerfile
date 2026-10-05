ARG BUILD_IMAGE=rust:1.96.1-slim-bookworm
# Optional local contexts contain only an installed compiler and public registry cache.
FROM scratch AS local_toolchain
FROM scratch AS local_registry
FROM ${BUILD_IMAGE} AS build
RUN apt-get update && apt-get install -y --no-install-recommends cmake make g++ pkg-config ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY bin ./bin
COPY data/fixtures ./data/fixtures
COPY docs/methodology.md ./docs/methodology.md
ARG BUILD_JOBS=2
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    --mount=from=local_toolchain,target=/opt/local-rust,ro \
    --mount=from=local_registry,target=/opt/local-registry,ro \
    if [ -x /opt/local-rust/bin/cargo ]; then \
      cp -a /opt/local-registry/cache /opt/local-registry/index /usr/local/cargo/registry/ && \
      CARGO_HOME=/usr/local/cargo RUSTC=/opt/local-rust/bin/rustc \
        /opt/local-rust/bin/cargo build --release --locked --offline -p alightd -j "${BUILD_JOBS}"; \
    else cargo build --release --locked -p alightd -j "${BUILD_JOBS}"; fi && \
    cp target/release/alightd /alightd

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 1000 alight && useradd --uid 1000 --gid 1000 --no-create-home alight
COPY --from=build /alightd /usr/local/bin/alightd
ARG GIT_REVISION=unknown
LABEL org.opencontainers.image.revision=${GIT_REVISION}
USER 1000:1000
WORKDIR /app
HEALTHCHECK --interval=30s --timeout=5s --start-period=45s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/v1/health | grep -q '"collector_status":"PASS"'
ENTRYPOINT ["/usr/local/bin/alightd"]
CMD ["--mode","observe"]
