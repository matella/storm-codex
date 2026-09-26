# Modèle de données — Postgres

Source de vérité : `crates/storm-codex-server/migrations/` (embarquées, exécutées au démarrage
par `sqlx::migrate!`). Stratégie : **JSONB pour la projection sans perte** (re-process
idempotent) + **colonnes scalaires promues** pour les axes de filtre/tri chauds (indexées).

## Cœur (0001, 0002)

| Table | Rôle | Points clés |
|---|---|---|
| `upload_tokens` | tokens d'upload nominatifs | `token_hash` = SHA-256 hex (jamais le clair) ; `revoked_at` |
| `uploads` | 1 ligne par fichier reçu (traçabilité) | `fingerprint` (contenu, UNIQUE), `archived_path`, `status` ∈ pending/parsed/duplicate/parse_failed, `error_class`/`error_msg`, `parser_version`, `match_id` → matches (0002) |
| `matches` | projection d'une partie | `fingerprint` (partie, UNIQUE), scalaires promus (build, mode, map, length, played_at, winner, first_*), **`data` = objet `match` storm-stats complet**, `parser_version` |
| `match_players` | 10 lignes par match | scalaires promus (hero, team, win, kills…), **`data` = objet `player` complet**, UNIQUE (match_id, toon_handle), CASCADE |
| `players` | référentiel joueurs agrégé | `toon_handle` PK, `last_name`, `names` (alias JSONB) — dénormalisé, best-effort, dérivable de `match_players` |

## Définitions manuelles (0001, 0004)

`teams` (roster JSONB de ToonHandles + colonne `league` texte — 0004), `leagues` (héritée,
inutilisée : le groupement se fait par `teams.league`), `collections` (match_ids JSONB).
Recréées à la main dans l'UI — **jamais** dérivées des replays.

## Référentiel répliqué (0001, 0005, 0006, 0007) — propriété du job `dim.rs`

| Table | Contenu | Clé de jointure |
|---|---|---|
| `dim_heroes` | 90 héros (nom, rôle, univers, data) | nom interne |
| `dim_talents` | ~1 900 talents | **`tree_id`** (0005) = `talentTreeId`, le même identifiant que `player.talents[TierNChoice]` écrit par le parser |
| `dim_patches` | liste des patch notes | `internal_id` ; détail proxifié à la demande |
| `patch_hero_sections` | sections héros des patchs (0007) | `hero_key` (nom normalisé) → liens héros ↔ patch, classification BUFF/NERF/MIXED |

Ces tables sont **reconstruisibles** (refresh complet au démarrage/24 h) — pas de donnée primaire.

## Réglages & draft (0003, 0008)

- `app_settings` (clé/valeur JSONB) : premier usage `operator_names` — les comptes de
  l'opérateur, pour afficher SA perspective (session, matches, widget, brief Jarvis).
- `draft_live` : singleton (id=1, CHECK) — tout l'état du simulateur de draft en JSONB
  (config, picks/bans, historique fearless), horodaté.

## Scouting (0010)

Rapports de scouting (spec `docs/specs/2026-09-26-rapports-scouting-design.md`). **Isolés** des
tables de stats personnelles : un replay adverse n'est jamais projeté dans `matches`.

| Table | Rôle | Points clés |
|---|---|---|
| `scouting_reports` | un rapport | `title`, `target_name`, `roster`/`anchors` (JSONB de toon_handles ; roster `[]` = détection auto), `facts` = instantané `{facts, detection, roster, roster_auto}` recalculé à chaque mutation, `facts_version`, `analysis` (JSON normalisé, une seule, écrasable) + `analysis_facts_version` / `analysis_model` / `analysis_imported_at` |
| `scouting_games` | un replay d'un rapport | UNIQUE `(report_id, fingerprint)`, `file_sha256`, `archived_path` (`ARCHIVE_DIR/scouting/`), `target_team` + `target_source` (`roster`/`anchor`/`manual`), **`summary`** = résumé compact (seule entrée du calcul des faits), `data` = sortie storm-stats complète, `parser_version` ; CASCADE depuis le rapport |

Le statut d'un rapport (`empty`/`ready`/`analyzed`/`stale`) est **dérivé**, jamais stocké.

## Conventions pour toute nouvelle migration

1. Fichier `NNNN_description.sql` séquentiel — **jamais** modifier une migration appliquée. Fins de
   ligne forcées en LF (`.gitattributes`) : sqlx vérifie la somme de contrôle des octets exacts, un
   checkout Windows en CRLF donnait `VersionMismatch` sur une base migrée en LF.
2. Grosse structure → JSONB ; axe de filtre/tri → colonne promue + index.
3. Si la projection change de forme : bump `PARSER_VERSION` (main.rs) → `POST /api/admin/reprocess`.
4. Mettre à jour **ce fichier** dans le même commit.
