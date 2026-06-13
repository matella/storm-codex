# storm-codex-server — image de prod (front + serveur dans un seul binaire/conteneur).
# Multi-stage : build du SPA (Vite) → build du serveur Rust (release) → runtime slim.
# Pas de DATABASE_URL au build (toutes les requêtes sqlx sont runtime) ; migrations embarquées
# (sqlx::migrate!) et exécutées au démarrage ; tables de protocole committées (pas de Python).

# ── 1. Front (web/dist) ─────────────────────────────────────────────────────
FROM node:22-slim AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# ── 2. Serveur Rust (release) ───────────────────────────────────────────────
FROM rust:1-slim AS server
WORKDIR /src
# dépendances d'abord (cache) : on copie les manifests du workspace + des crates
COPY Cargo.toml Cargo.lock ./
COPY crates/storm-replay/Cargo.toml crates/storm-replay/Cargo.toml
COPY crates/storm-stats/Cargo.toml crates/storm-stats/Cargo.toml
COPY crates/storm-codex-server/Cargo.toml crates/storm-codex-server/Cargo.toml
# sources
COPY crates/ crates/
RUN cargo build --release -p storm-codex-server

# ── 3. Runtime ──────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=server /src/target/release/storm-codex-server /usr/local/bin/storm-codex-server
COPY --from=web /web/dist /app/web
ENV WEB_DIR=/app/web \
    ARCHIVE_DIR=/data/archive \
    RAW_CACHE_DIR=/data/raw-cache \
    IMAGES_DIR=/data/images \
    BIND_ADDR=0.0.0.0:8088
EXPOSE 8088
# health : /api/health renvoie 200 si la DB répond
HEALTHCHECK --interval=15s --timeout=4s --start-period=20s --retries=4 \
  CMD curl -fsS http://localhost:8088/api/health || exit 1
CMD ["storm-codex-server"]
