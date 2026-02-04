FROM rust:1.84-bookworm AS builder

WORKDIR /app

# Cache deps
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs
RUN cargo build --release

# Build real binary
COPY src ./src
COPY migrations ./migrations
RUN cargo build --release

FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
  && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/vapor /app/vapor
COPY migrations /app/migrations

ENV APP_ADDR=0.0.0.0:3000
EXPOSE 3000

CMD ["/app/vapor", "serve"]

