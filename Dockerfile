FROM rust:1.95-bookworm AS builder

WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends libsqlite3-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY templates ./templates
COPY migrations ./migrations
RUN cargo build --locked --release

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/aboutme /usr/local/bin/aboutme
COPY static ./static
COPY content ./content

ENV ADDRESS=0.0.0.0:3000 \
    DATABASE_URL=/app/data/db.sqlite
EXPOSE 3000

CMD ["aboutme"]
