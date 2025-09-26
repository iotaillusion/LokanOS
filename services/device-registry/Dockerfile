# syntax=docker/dockerfile:1.6
FROM --platform=$BUILDPLATFORM rust:1.80-alpine AS builder

RUN apk add --no-cache build-base musl-dev openssl-dev pkgconfig curl
RUN rustup target add x86_64-unknown-linux-musl
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY services/device-registry/Cargo.toml services/device-registry/Cargo.toml
COPY services/api-gateway/Cargo.toml services/api-gateway/Cargo.toml
COPY services/rule-engine/Cargo.toml services/rule-engine/Cargo.toml
COPY services/scene-svc/Cargo.toml services/scene-svc/Cargo.toml
COPY services/presence-svc/Cargo.toml services/presence-svc/Cargo.toml
COPY services/energy-svc/Cargo.toml services/energy-svc/Cargo.toml
COPY services/updater/Cargo.toml services/updater/Cargo.toml
COPY services/telemetry-pipe/Cargo.toml services/telemetry-pipe/Cargo.toml
COPY services/audit-log/Cargo.toml services/audit-log/Cargo.toml
COPY services/radio-coord/Cargo.toml services/radio-coord/Cargo.toml
COPY crates/common-auth/Cargo.toml crates/common-auth/Cargo.toml
COPY crates/common-config/Cargo.toml crates/common-config/Cargo.toml
COPY crates/common-msgbus/Cargo.toml crates/common-msgbus/Cargo.toml
COPY crates/common-mdns/Cargo.toml crates/common-mdns/Cargo.toml
COPY crates/common-ble/Cargo.toml crates/common-ble/Cargo.toml
COPY crates/common-obs/Cargo.toml crates/common-obs/Cargo.toml
RUN cargo fetch --locked

COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release -p device-registry --target x86_64-unknown-linux-musl --no-default-features --features sqlite \
    && cp target/x86_64-unknown-linux-musl/release/device-registry target/x86_64-unknown-linux-musl/release/device-registry-sqlite \
    && cargo build --release -p device-registry --target x86_64-unknown-linux-musl --no-default-features --features postgres \
    && cp target/x86_64-unknown-linux-musl/release/device-registry target/x86_64-unknown-linux-musl/release/device-registry-postgres

RUN mkdir -p /tmp/device-registry-data \
    && chown 65532:65532 /tmp/device-registry-data

FROM gcr.io/distroless/static-debian11
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/device-registry-sqlite /usr/local/bin/device-registry-sqlite
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/device-registry-postgres /usr/local/bin/device-registry-postgres
COPY --from=builder --chown=65532:65532 /tmp/device-registry-data /var/lib/device-registry
COPY --from=builder /bin/busybox /bin/busybox
COPY --from=builder /etc/ssl /etc/ssl
COPY services/device-registry/docker-entrypoint.sh /usr/local/bin/device-registry-entrypoint.sh
RUN chmod 0755 /usr/local/bin/device-registry-entrypoint.sh
USER 65532:65532
WORKDIR /var/lib/device-registry
EXPOSE 8001
ENTRYPOINT ["/usr/local/bin/device-registry-entrypoint.sh"]
