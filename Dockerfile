# Stage 1: Build frontend
FROM node:22-slim AS frontend-builder
WORKDIR /app/frontend
RUN npm install -g bun
COPY frontend/package.json frontend/bun.lock* ./
RUN bun install --frozen-lockfile || bun install
COPY frontend/ .
RUN bun run build

# Stage 2: Build Rust backend
FROM rust:1-slim-bookworm AS backend-builder
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY tests/ tests/
COPY static/ static/
COPY SKILL.md ./
RUN cargo build --release

# Stage 3: Runtime
FROM debian:bookworm-slim AS runtime

LABEL org.opencontainers.image.source="https://github.com/Humans-Not-Required/watchpost"
LABEL org.opencontainers.image.description="Uptime monitoring and status pages for AI agents"
LABEL org.opencontainers.image.licenses="MIT"

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app

COPY --from=backend-builder /app/target/release/watchpost /app/watchpost
COPY --from=frontend-builder /app/frontend/dist /app/frontend/dist

ENV ROCKET_ADDRESS=0.0.0.0
ENV ROCKET_PORT=8000
ENV STATIC_DIR=/app/frontend/dist
ENV DATABASE_PATH=/app/data/watchpost.db

# Email notification SMTP config (optional — email notifications disabled if SMTP_HOST not set)
# ENV SMTP_HOST=smtp.example.com
# ENV SMTP_PORT=587
# ENV SMTP_USERNAME=user
# ENV SMTP_PASSWORD=pass
# ENV SMTP_FROM=watchpost@example.com
# ENV SMTP_TLS=starttls

EXPOSE 8000

VOLUME ["/app/data"]

CMD ["/app/watchpost"]
