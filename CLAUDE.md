# CLAUDE.md — Storm Codex

> Chargé à chaque session. **Commencer par `docs/STATUS.md`** (état + prochaine étape), le mettre
> à jour en fin de session. La spec programme est `docs/specs/2026-06-12-storm-codex-design.md` —
> elle a été validée par l'opérateur et relue : ne pas re-trancher ses décisions sans lui.

## Ce qu'est le projet
Rénovation complète de Stats of the Storm (tracker de stats HotS) : crates Rust open-source
(`storm-replay` décodage, `storm-stats` analyse), serveur unique `storm-codex-server` (axum +
Postgres + WS) sur le box, SPA React au design system Nexus Codex, widget stream OBS, intégration
Jarvis (événements Redis) et uploader `client-rs` existant (repo Hots-Overlay).

## Règles dures
1. **Un plan par jalon** (0→6 dans la spec), dans l'ordre ; chaque jalon a un critère
   d'acceptation mesurable — il n'est « fini » que critère vérifié.
2. **Jalon 0 d'abord** : spike go/no-go (décoder 3 replays réels en Rust, bench 50 fichiers vs
   .NET/Python ; accept < 500 ms/replay). Si no-go → repli .NET documenté, même architecture.
3. **Parité stats prouvée, pas déclarée** : storm-stats est validé par diff automatique contre
   hots-parser (Node) sur un corpus de référence. Pas de jalon 2 sans diff vert.
4. **Budgets perf de la spec** = contrats : parse < 150 ms/replay, fin de partie → page < 5 s,
   API p95 < 100 ms, backfill 3 ans < 5 min. Mesurer, pas estimer.
5. **3 étages de données** : replay brut archivé (source de vérité) ; Postgres = projection
   complète (`parser_version` partout, re-process idempotent) ; dump décodé à la volée + cache
   LRU. Jamais de pré-décodage massif.
6. Événements vers Jarvis : invariants du spine (schema_version, correlation_id/causation_id,
   occurred_at/recorded_at, `entity.verb` au passé).
7. Le référentiel héros/talents vient de l'API HotsPatchNotes (répliqué en `dim_*` au démarrage) —
   pas de second pipeline de sync.

## Environnement
- Box : matella@192.168.129.85 (serveur du soir ~18h→nuit) ; Postgres/Redis Jarvis dessus ;
  HotsPatchNotes api :5001 / web :5100 ; overlay Node :8086 (à décommissionner au jalon 5,
  l'EBS Twitch Azure reste). Déploiement : rsync + docker compose build (pas de `git pull` box).
- PC de jeu Windows : replays dans `Documents/Heroes of the Storm/Accounts/*/*/Replays/Multiplayer`
  (~3 000 fichiers, 2–3 GB) ; uploader = `client-rs` (repo Hots-Overlay).
- Corpus de tests : NAS/box + script `fetch-corpus` ; mini-corpus committé pour CI publique.

## Conventions
Rust 2021+, clippy strict, erreurs typées (`thiserror`), pas de `unwrap()` hors tests ;
front React + Vite + TS + TanStack + uPlot, tokens Nexus Codex ; commits conventionnels ;
secrets uniquement dans le .env du box, jamais commités.
