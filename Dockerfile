# syntax=docker/dockerfile:1
FROM rust:1-slim-bookworm AS builder
RUN rustup target add wasm32-unknown-unknown \
    && cargo install trunk --locked

WORKDIR /app
COPY Cargo.toml ./
COPY common ./common
COPY backend ./backend
COPY frontend ./frontend

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release -p backend \
    && cp target/release/backend /app/backend-bin

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cd frontend && trunk build --release

# distroless/cc ships glibc + ca-certificates + a built-in non-root user.
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
WORKDIR /app
COPY --from=builder /app/backend-bin ./backend
COPY --from=builder /app/frontend/dist ./frontend/dist

EXPOSE 3000
ENTRYPOINT ["./backend"]
CMD ["--static-dir", "frontend/dist"]
