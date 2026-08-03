# Spec vivante — Storm Codex

Documentation **vivante** du projet : elle décrit l'état **courant** des contrats (API, données,
événements, invariants), pas l'histoire. C'est la référence à lire avant de modifier un
comportement, et à **mettre à jour dans le même commit** que le changement.

## Règle de maintenance (le « vivant » de la spec)

> **Tout commit qui change un contrat documenté ici met à jour la section correspondante.**
> Un endpoint ajouté/modifié → `05-api.md`. Une migration → `06-modele-donnees.md`. Un event
> WS/Redis → `07-evenements.md`. Une page/route SPA → `08-frontend.md`. Une variable d'env ou
> une étape de déploiement → `04-serveur.md` / `09-operations.md`.
> Le code reste la source de vérité de détail ; la spec vivante documente le **contrat et le
> pourquoi** — si les deux divergent, c'est un bug de doc à corriger dans le commit qui le révèle.

## Cartographie documentaire (qui fait quoi)

| Emplacement | Nature | Cycle de vie |
|---|---|---|
| `docs/spec/` (**ici**) | contrats & invariants **courants** | vivant — maintenu à chaque changement |
| `docs/STATUS.md` | journal d'avancement, prochaine étape | vivant — mis à jour en fin de session |
| `docs/specs/` (datées) | décisions de conception validées par l'opérateur | figées — ne pas re-trancher sans lui |
| `docs/plans/` | plans de jalons exécutés | archives |
| `docs/research/` | preuves : benchs, rapports de parité, références | archives (référencées ici) |
| `docs/runbooks/` | procédures opérateur pas-à-pas | maintenus au besoin |
| `crates/*/README.md` | vitrine + contrat public de chaque crate (publication crates.io) | vivants |

## Sommaire

1. [Architecture & invariants](01-architecture.md) — composants, 3 étages de données, budgets perf
2. [storm-replay](02-storm-replay.md) — contrat du décodeur de replays
3. [storm-stats](03-storm-stats.md) — contrat des stats, parité hots-parser, tolérances
4. [Serveur](04-serveur.md) — pipeline upload→parse→projection, config env, référentiel
5. [API](05-api.md) — référence REST + WebSocket
6. [Modèle de données](06-modele-donnees.md) — schéma Postgres (migrations)
7. [Événements](07-evenements.md) — WS `/ws` et contrat Jarvis (Redis)
8. [Frontend](08-frontend.md) — pages SPA, overlays OBS
9. [Opérations](09-operations.md) — dev, déploiement box, CI, sauvegardes, pièges
10. [**Workflow de développement**](10-developpement.md) — **à lire avant toute modification** :
    cycle spec-first, checklist doc/preuve par type de changement, définition de « fini »

## Carte du repo

| Chemin | Contenu | Statut |
|---|---|---|
| `crates/storm-replay` · `storm-stats` | bibliothèques publiques (décodage, stats) | actif — publiables crates.io |
| `crates/storm-codex-server` | serveur (API + front + overlays) + migrations | actif |
| `web/` | SPA React (Vite) buildée dans `web/dist` | actif |
| `tools/` | `protocol_gen.py` (tables de protocole), `crosscheck_streams.py` (parité décodage), `parity-harness/` (parité stats vs hots-parser) | outillage — requis pour les preuves |
| `corpus/` | replays réels de test (`spike50` bench, `stats` parité) — **non committés** (gitignorés, NAS/box) | données locales |
| `spike/` | jalon 0 (spike go/no-go décodage, benchs .NET/Python) | archive — ne plus toucher |
| `scripts/` | `backup.sh` (pg_dump + rétention) | actif |
| `.github/workflows/` | `ci.yml` (clippy + tests Rust/web, Postgres de service) · `publish.yml` (image Docker → ghcr.io à chaque push `main`) | actifs |
| `docs/` | voir cartographie ci-dessus | — |
