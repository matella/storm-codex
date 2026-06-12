# STATUS — lire d'abord, mettre à jour en dernier

## Où on en est (2026-06-12)
- **Recherche terminée** : anatomie SotS + hots-parser, verdicts dépendances, comparatif moteurs,
  écosystème (HeroesProfile vivant, HeroesMatchTracker archivé). → `docs/research/`.
- **Spec programme validée** (opérateur + revue agent, 2 passes) : Rust core, Postgres,
  3 étages de données, parité SotS totale en V1 (ligues incluses), remplacement du serveur Node
  local, widget stream, scope C (auto-hébergé V1, open-source, public-ready V2), multi-tokens.
  → `docs/specs/2026-06-12-storm-codex-design.md`.
- **Maquettes validées** (14 écrans, couverture vue-par-vue auditée contre SotS ; widget défaite
  enrichi sur retour opérateur). → `docs/specs/2026-06-12-storm-codex-mockup.html`.
- **Jalon 0 : GO** (2026-06-12, exécuté sur le PC de jeu). Spike Rust `spike/storm-decode` :
  50/50 replays décodés (header+details+tracker), médiane **12 ms** / p95 21 ms vs .NET 37 ms
  (1 échec) et Python 161 ms ; cross-check heroprotocol 3/3 identique. Repli .NET écarté.
  → rapport : `docs/research/2026-06-12-jalon0-bench.md` · plan exécuté :
  `docs/plans/2026-06-12-jalon0-spike-decode.md`.

- **Jalon 1 : FAIT** (2026-06-12). Crate `crates/storm-replay` : 7 streams décodés (versioned +
  bitpacked + attributes), tables protocol-gen embarquées (390 builds → 32 dédupliquées),
  fallback builds inconnus signalé, erreurs typées. **Critères : archive réelle 2 821/2 821
  décodés (100 %, 22 builds 2024→2026) ; bench 7 streams médiane 102 ms < 150** (p95 205 ms,
  dont ~50–115 ms de plancher décompression bzip2 incompressible — point signalé à l'opérateur).
  Parité **bit-exacte** prouvée vs heroprotocol (deep-compare 7 streams × 4 replays,
  `tools/crosscheck_streams.py`). Plan : `docs/plans/2026-06-12-jalon1-storm-replay.md`.

- **Jalon 2 : FAIT** (2026-06-12). Crate `crates/storm-stats` : port fidèle de hots-parser
  (3 360 lignes JS) → `{match, players, status}`. **Critère : diff automatique champ par champ
  vs hots-parser 7.55.7 (Node) — 114/114 verts** (79 parse complet identique, 35 rejets
  identiques sur cartes absentes de la `MapType` de la référence). 1 tolérance documentée et
  favorable (coordonnées de ping : storm-stats plus correct que la référence). Bench parse
  complet **133 ms médiane** (échantillon représentatif, < 150 budget ; à-budget 151 ms sur le
  pire cas, décodage-dominé). Harnais : `tools/parity-harness/` (`dump.js`, `diff.py`,
  `tolerances.json`). Rapport : `docs/research/2026-06-12-jalon2-parite.md` · plan :
  `docs/plans/2026-06-12-jalon2-storm-stats.md`.

## Prochaine étape — Jalon 3 : serveur + Postgres + backfill (plan à écrire, puis exécuter)
1. Plan writing-plans dédié. `storm-codex-server` (axum) : `POST /api/upload` (token, fingerprint
   anti-doublon, archive d'abord, pool de workers parse storm-replay→storm-stats, transaction
   Postgres, événement), REST de lecture, WebSocket `match.parsed`, dump `…/raw` + cache LRU,
   Admin (tokens, re-process idempotent piloté par `parser_version`, santé). Mode backfill
   `client-rs`.
2. *Accept : 100 % de l'archive archivée et tentée ; ≥ 99 % parsée (échecs listés/classés dans
   Admin) ; fin de partie → page < 5 s.* Budgets : écriture PG + push WS < 200 ms,
   backfill 3 ans < 5 min (workers = nb cœurs).
3. **Box requis** (Postgres/Redis Jarvis, soir ~18h→nuit, Tailscale 192.168.129.85) — premier
   jalon qui en dépend (jalons 0–2 étaient autosuffisants sur le PC de jeu).
4. Acquis réutilisables : `storm-stats::process_replay`, `Replay::visit_game_events`,
   feature `fast-alloc`, classes d'erreur typées `storm_replay::Error` (→ `uploads.error_class`).

## Jalons (résumé — détail et critères dans la spec)
0 spike **GO ✅** → 1 storm-replay **✅** → 2 storm-stats **✅** →
3 serveur+DB+backfill → 4 front parité → 5 stream+Jarvis+bascule (décommission Node local) →
6 publication crates.

## Décisions verrouillées (ne pas rouvrir sans l'opérateur)
Rust (**confirmé par le spike** — repli .NET écarté) · Postgres · design Nexus Codex ·
remplacement serveur Node local (EBS Twitch Azure conservé en V1, alimenté par push) · V1 =
parité totale · pas de pré-game · aucune migration de données (backfill + recréation manuelle)
· nom Storm Codex.

## Bloquants / besoins opérateur
- ~~Échantillon de replays pour le jalon 0~~ — résolu : le jalon 0 a tourné sur le PC de jeu
  (2 652 replays locaux, corpus reproductible via `spike/sample_corpus.ps1`).
- Création des repos publics GitHub (storm-replay/storm-stats) au moment du jalon 6 — d'ici là,
  tout vit dans ce repo.
