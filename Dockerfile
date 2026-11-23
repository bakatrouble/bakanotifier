# syntax=docker/dockerfile:1

FROM rust:alpine AS builder

RUN apk add --no-cache build-base cmake linux-headers perl pkgconf

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    mkdir -p src \
    && printf 'fn main() {}\n' > src/main.rs \
    && cargo fetch --locked

COPY build.rs ./
COPY src ./src
COPY schemas ./schemas
COPY templates ./templates

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release --locked \
    && cp target/release/bakanotifier /app/bakanotifier

FROM alpine:3.22

RUN apk add --no-cache ca-certificates \
    && mkdir -p /app /config

COPY --from=builder /app/bakanotifier /app/bakanotifier
COPY templates /templates
COPY config.docker.example.toml /config/config.example.toml

WORKDIR /app
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/app/bakanotifier", "healthcheck", "--address", "127.0.0.1:8080"]

ENTRYPOINT ["/app/bakanotifier"]
CMD ["--config", "/config/config.toml"]
