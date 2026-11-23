# syntax=docker/dockerfile:1

FROM rust:alpine AS builder

RUN apk add --no-cache build-base cmake linux-headers perl pkgconf

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
    && printf 'pub fn placeholder() {}\n' > src/lib.rs \
    && printf 'fn main() {}\n' > src/main.rs \
    && cargo fetch --locked \
    && cargo build --release --locked

COPY src ./src
COPY templates ./templates
RUN touch src/lib.rs src/main.rs \
    && cargo build --release --locked

FROM alpine:3.22

RUN apk add --no-cache ca-certificates \
    && addgroup -S bakanotifier \
    && adduser -S -G bakanotifier bakanotifier \
    && mkdir -p /etc/bakanotifier

COPY --from=builder /app/target/release/bakanotifier /usr/local/bin/bakanotifier
COPY templates /etc/bakanotifier/templates
COPY config.docker.example.toml /etc/bakanotifier/config.example.toml

USER bakanotifier
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/usr/local/bin/bakanotifier", "healthcheck", "--address", "127.0.0.1:8080"]

ENTRYPOINT ["/usr/local/bin/bakanotifier"]
CMD ["--config", "/etc/bakanotifier/config.toml"]
