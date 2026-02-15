FROM rust:slim AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY shared/ shared/
COPY api/ api/

# Create dummy interactive crate so workspace resolves
RUN mkdir -p interactive/src && \
    echo '[package]\nname = "mikaana-interactive"\nversion = "0.1.0"\nedition = "2021"\n' > interactive/Cargo.toml && \
    echo 'fn main() {}' > interactive/src/main.rs

RUN cargo build --release -p mikaana-api

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/mikaana-api /usr/local/bin/mikaana-api

EXPOSE 8080

CMD ["mikaana-api"]
