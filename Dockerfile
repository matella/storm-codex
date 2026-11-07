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
# cargo-chef : les dépendances du workspace (5 crates) sont compilées dans une couche à part, qui
# ne se reconstruit que si Cargo.toml/Cargo.lock changent — un changement de code ne recompile plus
# tout l'arbre de dépendances.
FROM rust:1-slim AS chef
RUN cargo install cargo-chef --locked --version ^0.1
WORKDIR /src

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS server
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release -p storm-codex-server --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cargo build --release -p storm-codex-server

# ── 3. Runtime ──────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home-dir /app --shell /usr/sbin/nologin storm \
 && mkdir -p /data \
 && chown storm:storm /data
WORKDIR /app
COPY --from=server /src/target/release/storm-codex-server /usr/local/bin/storm-codex-server
COPY --from=web /web/dist /app/web
# minimaps in-game bakées (committées au repo) — copiées dans IMAGES_DIR/minimaps au démarrage
COPY assets/minimaps /app/minimaps
ENV WEB_DIR=/app/web \
    ARCHIVE_DIR=/data/archive \
    RAW_CACHE_DIR=/data/raw-cache \
    IMAGES_DIR=/data/images \
    MINIMAPS_BUNDLE_DIR=/app/minimaps \
    ORPHEUS_URL=http://host.docker.internal:3010 \
    BIND_ADDR=0.0.0.0:8088
EXPOSE 8088
# Utilisateur non root. Un volume /data créé par une ancienne image (root) est remis à cet uid par
# le service `storm-codex-init` de docker-compose.yml avant le démarrage du serveur.
USER 10001:10001
# health : /api/health renvoie 200 si la DB répond
HEALTHCHECK --interval=15s --timeout=4s --start-period=20s --retries=4 \
  CMD curl -fsS http://localhost:8088/api/health || exit 1
CMD ["storm-codex-server"]
