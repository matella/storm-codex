# Jalon 4 — front parité (SPA React, design Nexus Codex)

> **For Claude:** REQUIRED SUB-SKILL: superpowers:executing-plans. Context7 avant React/Vite/
> TanStack/uPlot si doute d'API.

**Goal :** SPA React + Vite + TS servie par `storm-codex-server`, consommant son API REST + WS,
au design **Nexus Codex** (tokens de la maquette), couvrant les pages SotS data-backed.
**Accept (spec) :** chaque fonctionnalité SotS retrouvable (par page), p95 API < 100 ms (déjà
tenu côté serveur), temps réel (nouveau match en tête sans F5).

**Design (maquette validée `docs/specs/2026-06-12-storm-codex-mockup.html`) :** fond #06070b,
surfaces #0e1016/#16181f, hairlines #1a1d2a/#232636, accent #7F77DD, victoire #5DCAA5,
défaite #E24B4A/#F7C1C1, équipe bleue #85B7EB / rouge #F09595, anneaux d'univers
(Warcraft #EF9F27, StarCraft #378ADD, Diablo #E24B4A, Overwatch #D85A30, Nexus #AFA9EC),
Inter + JetBrains Mono, badges `.bdg`, pills, kickers, avatars initiales.

**Stack :** Vite + React 18 + TS, TanStack Query (cache + invalidation WS), react-router,
uPlot (timelines). Build statique servi par le binaire (tower-http ServeDir, fallback index.html).

---

### Task 1 : scaffold Vite + design tokens + layout + service API
- `web/` : Vite React-TS, `package.json`, `vite.config.ts` (build → `web/dist`, proxy `/api`+`/ws`
  vers :8088 en dev). `src/theme.css` : tokens Nexus Codex + composants (.bdg/.pill/.av/.kick…).
- `src/api.ts` : fetch typé (`/api/matches`, `/matches/{id}`, `/players/{toon}`, `/heroes`) +
  hook WS (`/ws` → invalide les queries matches). Layout (header nav + live-dot).
- Build OK, page blanche stylée. Commit.

### Task 2 : Matchs (liste filtrable temps réel) + Dashboard session
- Page Matchs : liste depuis `/api/matches` (filtres mode/carte/héros), lignes au style maquette
  (heure, badge mode, avatar héros à anneau d'univers, KDA, V/D), pagination, **WS → nouveau
  match en tête** (toast « replay reçu »). Dashboard : dernière partie + stats session.
- Vérifié contre la DB backfillée (réelle). Commit.

### Task 3 : Détail de match
- `/match/{id}` depuis `/api/matches/{id}` : en-tête (carte, mode, date, build), draft
  (bans/picks ordonnés), score 10 joueurs (2 équipes, ~80 stats via le JSONB), timelines
  XP/niveaux + objectifs (uPlot), taunts/BM, chat. Commit.

### Task 4 : Joueur + Héros + Cartes
- `/player/{toon}` (résumé, hero pool, winrate) ; `/heroes` (tableau agrégé games/wins/winrate,
  tri) + `/hero/{nom}` ; `/maps` (winrate par carte). Tables TanStack triables. Commit.

### Task 5 : service statique côté serveur + build intégré + STATUS + push
- `storm-codex-server` sert `web/dist` (ServeDir + fallback `index.html` pour le routing SPA).
  `npm run build` → bundle ; le binaire le sert sur `/`. Vérif end-to-end (serveur + DB +
  front buildé). Checklist parité par page. STATUS, commit, **push**.

---

## Pièges
- Anneaux d'univers : besoin de l'univers du héros → `dim_heroes` (jalon 4 : peupler depuis
  l'API HotsPatchNotes :5001 du box au démarrage du serveur ; si indispo, fallback couleur accent).
- Le score screen (~80 stats) est dans `match_players.data.gameStats` (JSONB) — afficher un
  sous-ensemble lisible + table complète repliable.
- Routing SPA : le serveur doit renvoyer index.html sur les routes front inconnues (pas 404).
- Temps réel : le WS pousse `{type:match.parsed, match_id, map}` → invalider la query matches.
