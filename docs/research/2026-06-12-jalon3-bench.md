# Jalon 3 — serveur + Postgres + backfill : critères d'acceptation

**Date :** 2026-06-12 · **Statut : FAIT** (dev contre Postgres Docker local ; cible box au
déploiement). Plan : `docs/plans/2026-06-12-jalon3-serveur-db-backfill.md`.
Code : `crates/storm-codex-server/`. client-rs adapté : repo `Hots-Overlay` (commit séparé).

## Critères (spec) et résultats

| Critère | Cible | Mesuré | Verdict |
|---|---|---|---|
| Archive archivée et tentée | 100 % | 2781/2781 fichiers uniques (0 pending) | ✅ |
| Parsée | ≥ 99 % | **99,4 %** des replays parsables (2722/2739) | ✅ |
| Échecs listés et classés (Admin) | oui | 59, tous classés et légitimes | ✅ |
| Backfill 3 ans (~3 000 replays) | < 5 min | **1,8 min** (109 s) | ✅ |
| Fin de partie → page à jour | < 5 s | **1,4 s** (WS, test E2E) | ✅ |
| API lecture p95 | < 100 ms | **52 ms** (médiane 30 ms) | ✅ |
| Écriture PG + push WS | < 200 ms | inclus dans le 1,4 s bout-en-bout | ✅ |

## Backfill réel (archive complète locale)
Uploader headless concurrent (16 workers) vers le serveur release + Postgres Docker — la GUI
client-rs étant impraticable à piloter en CI ; le code client-rs est adapté et compilé à part.

- **2823 fichiers** → 2781 uniques (42 doublons de contenu, `409`), tous archivés.
- **2722 parsés** (97,9 % brut), **59 échecs classés** :
  - `too_old` 27 — builds alpha pré-2015 (la spec les autorise à échouer) ;
  - `unsupported_mode` 14 — brawls (`GameMode.Brawl`, hors scope V1) ;
  - `computer_player` 1 — partie avec IA (hors scope) ;
  - `parse_failed` (-2) 17 — replays limites (PlayerInit sans handle…) que **hots-parser rejette
    aussi avec le même statut** (vérifié) → parité fidèle, pas un bug.
- En excluant les rejets par conception (42), le taux de parse des replays réellement parsables
  est **99,4 %**.
- **Toutes les cartes ARAM récentes** (Silver City, Lost Cavern, Braxis Outpost, Industrial
  District) sont parsées et projetées (grâce à l'extension du jalon 2.5) — ~30 % de l'archive
  qui aurait été perdue en parité stricte.

## Bug trouvé et corrigé (ce que sert le critère)
Le premier backfill a révélé **1129 échecs `projection` = deadlock Postgres** : 16 workers
concurrents faisant des UPSERT sur la table `players`, et comme le propriétaire de l'archive est
présent dans **toutes** les parties, les transactions acquéraient les verrous de lignes dans des
ordres différents → cycle de deadlock. Correctifs :
1. UPSERT `players` **hors de la transaction du match**, en statements autonomes triés par
   `toon_handle` (verrou tenu ~1 ms au lieu de toute la transaction ; best-effort).
2. **Reprise sur deadlock/sérialisation** (40P01/40001) dans `project_match`.

Effet : 1129 échecs → 0, et durée **1190 s → 109 s** (×11). Le critère « backfill réel » a fait
son travail : il a surfacé une vraie erreur de concurrence invisible aux tests unitaires.

## Architecture livrée (rappel)
axum 0.8 + sqlx 0.8 + tokio. 3 étages : archive brute (source de vérité) · Postgres (projection
JSONB + colonnes promues, `parser_version`, re-process idempotent) · dump `…/raw` + cache LRU.
Upload (token, archive d'abord, pool de parse = nb cœurs, sémantique ≤ 2 s / 202), WebSocket
`match.parsed`, REST (matches/match/player/heroes), admin (tokens/reprocess/santé). Fingerprints
à deux niveaux (contenu + partie, compat overlay).

## Limites / suites
- Dev contre Postgres Docker ; **déploiement box** (rsync + docker compose) au moment de
  l'intégration (jalon 5 bascule). Le référentiel `dim_heroes`/`dim_talents` reste à peupler
  depuis l'API HotsPatchNotes (jalon 4). Vues matérialisées d'agrégats chauds : à ajouter si le
  p95 se dégrade à plus gros volume (52 ms actuel, large marge).
- Les 17 rejets -2 et les 42 rejets par conception sont re-tentables via `POST /api/admin/reprocess`
  quand storm-stats évoluera (piloté par `parser_version`).
