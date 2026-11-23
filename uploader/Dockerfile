# Image uploader HEADLESS (Docker). Context = uploader/. Le binaire lie encore l'UI (eframe/tray)
# mais ne l'ouvre jamais en HEADLESS=1 → on installe juste les libs runtime pour qu'il se charge.
# (Slimming futur : feature-gate l'UI derrière une feature `gui` pour retirer ces deps.)
FROM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
    libgtk-3-dev libxdo-dev libssl-dev pkg-config libayatana-appindicator3-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    libgtk-3-0 libxdo3 libssl3 ca-certificates libayatana-appindicator3-1 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/hots-replay-client /usr/local/bin/uploader
ENV HEADLESS=1
# Config attendue à l'exécution : SERVER_URL, AUTH_TOKEN, REPLAY_DIR (monté en bind).
ENTRYPOINT ["/usr/local/bin/uploader"]
