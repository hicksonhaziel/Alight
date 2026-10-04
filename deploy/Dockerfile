FROM rust:1.96.1-slim-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends cmake make g++ pkg-config ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY bin ./bin
COPY data/fixtures ./data/fixtures
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked -p alightd -j 2 && cp target/release/alightd /alightd

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 1000 alight && useradd --uid 1000 --gid 1000 --no-create-home alight
COPY --from=build /alightd /usr/local/bin/alightd
USER 1000:1000
WORKDIR /app
HEALTHCHECK --interval=30s --timeout=5s --start-period=45s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/v1/health | grep -q '"collector_status":"PASS"'
ENTRYPOINT ["/usr/local/bin/alightd"]
CMD ["--mode","observe"]
