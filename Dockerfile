# storm-codex-server — production image (front-end + server in a single binary/container).
# Multi-stage: SPA build (Vite) → Rust server build (release) → slim runtime.
# No DATABASE_URL at build time (all sqlx queries are runtime); migrations embedded
# (sqlx::migrate!) and run at startup; protocol tables committed (no Python).

# ── 1. Front (web/dist) ─────────────────────────────────────────────────────
FROM node:22-slim AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# ── 1b. Rust front-end (web-rs/dist: overlay, sources, app) ─────────────────
# Trunk is a prebuilt binary (pinned: the version the bundles are checked with); it downloads its own wasm-bindgen
# and wasm-opt at build time. The three bundles are built into web-rs/dist by their Trunk.toml.
FROM rust:1-slim AS web-rs
RUN rustup target add wasm32-unknown-unknown \
 && apt-get update \
 && apt-get install -y --no-install-recommends curl ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && curl -fsSL https://github.com/trunk-rs/trunk/releases/download/v0.22.0-beta.1/trunk-x86_64-unknown-linux-gnu.tar.gz \
    | tar -xz -C /usr/local/bin
WORKDIR /src/web-rs
COPY web-rs/ ./
RUN for b in overlay sources app; do (cd "$b" && trunk build --release) || exit 1; done

# ── 2. Serveur Rust (release) ───────────────────────────────────────────────
# cargo-chef: the workspace dependencies (5 crates) are compiled in a separate layer, which
# is only rebuilt when Cargo.toml/Cargo.lock change — a code change no longer recompiles
# the whole dependency tree.
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
COPY --from=web-rs /src/web-rs/dist /app/web-rs
# baked in-game minimaps (committed to the repo) — copied into IMAGES_DIR/minimaps at startup
COPY assets/minimaps /app/minimaps
# WEB_ROOT=rust: the Rust front-end answers at `/` (the switch). The React build stays in the image for rollback:
# set WEB_ROOT=react (or remove it) and restart.
ENV WEB_DIR=/app/web \
    WEB_V2_DIR=/app/web-rs \
    WEB_ROOT=rust \
    ARCHIVE_DIR=/data/archive \
    RAW_CACHE_DIR=/data/raw-cache \
    IMAGES_DIR=/data/images \
    MINIMAPS_BUNDLE_DIR=/app/minimaps \
    ORPHEUS_URL=http://host.docker.internal:3010 \
    BIND_ADDR=0.0.0.0:8088
EXPOSE 8088
# Non-root user. A /data volume created by an older (root) image is handed to this uid by
# the `storm-codex-init` service of docker-compose.yml before the server starts.
USER 10001:10001
# health: /api/health returns 200 if the DB responds
HEALTHCHECK --interval=15s --timeout=4s --start-period=20s --retries=4 \
  CMD curl -fsS http://localhost:8088/api/health || exit 1
CMD ["storm-codex-server"]
