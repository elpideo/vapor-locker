FROM rust:1.88-bookworm AS builder

WORKDIR /app

# Cache deps
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs
RUN cargo build --release

# Build real binary (rm forces relink, avoids stale placeholder from cache)
COPY src ./src
COPY migrations ./migrations
RUN rm -f target/release/vapor target/release/deps/vapor target/release/deps/vapor-* 2>/dev/null; cargo build --release

FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
  && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/vapor /app/vapor
COPY migrations /app/migrations
COPY static /app/static

ENV APP_ADDR=0.0.0.0:3000
EXPOSE 3000

CMD ["/app/vapor", "serve"]

