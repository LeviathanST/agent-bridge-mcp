FROM rust:1.88-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/

RUN cargo build --release

FROM gcr.io/distroless/cc-debian12

COPY --from=builder /build/target/release/agent-bridge /usr/local/bin/agent-bridge

ENV RUST_LOG=info

EXPOSE 3000 9100

ENTRYPOINT ["agent-bridge"]
CMD ["--sse-port", "3000", "--ws-port", "9100", "--db-path", "/data/bridge.db"]
