# Architecture & invariants

## Vue d'ensemble

```
PC de jeu ──client-rs (Hots-Overlay)──► storm-codex-server (box :5102)
                POST /api/upload-raw          │ axum + Postgres + WS
                                              │
        ┌───────────────┬─────────────────────┼──────────────────┬──────────────┐
        ▼               ▼                     ▼                  ▼              ▼
   SPA React       overlays OBS         Redis Jarvis        HotsPatchNotes   archive brute
   (servie par     (/widget /queue      (PUBLISH match      (:5001, source   (.StormReplay,
    le binaire)     /draft/overlay…)     completed)          dim_*)           source de vérité)
```

Un **seul binaire** (`storm-codex-server`) sert l'API, le front buildé et les overlays. Les crates
`storm-replay` (décodage) et `storm-stats` (analyse) sont des bibliothèques pures, publiables
indépendamment (MIT).

## Les 3 étages de données (règle dure n° 5)

1. **Archive brute** (`ARCHIVE_DIR`) : chaque `.StormReplay` reçu est conservé tel quel —
   **source de vérité**. Jamais supprimé par le pipeline.
2. **Postgres** : projection **complète** du parse (`matches.data` + `match_players.data` JSONB,
   colonnes scalaires promues pour les filtres). `parser_version` partout → re-process
   **idempotent** (delete-then-insert par fingerprint). La base est reconstruisible depuis l'archive.
3. **Dump décodé à la volée** (`GET /api/matches/{id}/raw?stream=…`) + **cache disque LRU borné**
   (`RAW_CACHE_MAX_BYTES`). **Jamais de pré-décodage massif.**

## Invariants (à ne pas casser)

- **Parité prouvée, pas déclarée** : tout écart de storm-stats vs hots-parser est une tolérance
  documentée (`tools/parity-harness/tolerances.json`, [03-storm-stats.md](03-storm-stats.md)).
- **Idempotence** : re-uploader/re-processer un replay ne crée jamais de doublon
  (fingerprints à deux niveaux, cf. [04-serveur.md](04-serveur.md)).
- **Événements spine Jarvis** : `schema_version`, `correlation_id`/`causation_id`,
  `occurred_at`/`recorded_at`, type `entity.verb` au passé ([07-evenements.md](07-evenements.md)).
- **Best-effort pour tout ce qui est périphérique** : panne Redis, HotsPatchNotes absent,
  webhook mort — le pipeline upload→parse→projection ne doit jamais en dépendre.
- **Référentiel unique** : héros/talents/patches viennent de HotsPatchNotes (ou de son snapshot),
  répliqués en `dim_*` au démarrage — pas de second pipeline de sync.
- **`PARSER_VERSION`** (`main.rs`) : bumper à chaque changement de projection ; c'est lui qui
  pilote le re-process.

## Budgets de performance (contrats, mesurés — pas estimés)

| Budget | Contrat | Dernière mesure |
|---|---|---|
| Parse complet (decode + stats) | < 150 ms/replay | 133 ms médiane (`docs/research/2026-06-12-jalon2-parite.md`) |
| Fin de partie → page à jour | < 5 s | 1,4 s (`docs/research/2026-06-12-jalon3-bench.md`) |
| API p95 | < 100 ms | 52 ms (idem) |
| Backfill archive 3 ans (~2 800) | < 5 min | 1,8 min (idem) |

Toute PR qui risque un budget re-mesure (benchs : `storm-stats-dump --bench`,
`backfill_bench.py`).

## Décisions verrouillées (opérateur — ne pas rouvrir sans lui)

Rust (spike GO, repli .NET écarté) · Postgres · design Nexus Codex · remplacement du serveur
Node local · V1 = parité SotS totale · pas de pré-game · pas de migration de données (backfill) ·
**overlay local uniquement** (extension Twitch/Azure abandonnée, `azure.rs` dormant) ·
nom Storm Codex. Détail et rationale : `docs/specs/` (datées).
