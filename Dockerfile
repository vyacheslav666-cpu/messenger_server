FROM rust:1.78-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.toml
COPY src src
RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/messenger_server /app/messenger_server
COPY static /app/static
ENV PORT=8080
ENV DATABASE_PATH=/app/data/chat.db
EXPOSE 8080
CMD ["/app/messenger_server"]
