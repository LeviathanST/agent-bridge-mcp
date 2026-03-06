FROM rust:1.87-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/

RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/agent-bridge /usr/local/bin/agent-bridge

RUN mkdir -p /data

ENV RUST_LOG=info

EXPOSE 3000

ENTRYPOINT ["agent-bridge"]
CMD ["--sse-port", "3000", "--db-path", "/data/bridge.db"]
