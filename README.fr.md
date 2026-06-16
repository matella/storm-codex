**Français** · [English](README.md)

# Storm Codex

Une rénovation complète de [Stats of the Storm](https://github.com/ebshimizu/stats-of-the-storm) —
un tracker de stats de replays Heroes of the Storm — reconstruit en :

- **`storm-replay`** — une crate Rust qui décode les fichiers `.StormReplay` (MPQ + protocole
  bit-packed versionné, tables générées depuis [Blizzard/heroprotocol](https://github.com/Blizzard/heroprotocol)).
- **`storm-stats`** — une crate Rust qui transforme les replays décodés en stats riches : timelines
  d'objectifs par carte, teamfights, avantage XP/niveau, détection des taunts/BM, takedowns enrichis,
  récompenses — un portage complet de la logique de [hots-parser](https://github.com/ebshimizu/hots-parser).
- **`storm-codex-server`** — un serveur axum mono-binaire : upload de replays authentifié, workers de
  parsing parallèles, projections PostgreSQL, push WebSocket (fin de partie → page mise à jour en < 5 s),
  backfill de l'archive complète, décodage du flux brut à la demande.
- **Un SPA rapide** (React + Vite) avec une parité fonctionnelle complète avec Stats of the Storm —
  parties, détail de partie, joueurs, héros, builds de talents, compositions, tendances par patch,
  équipes & ligues, collections, cartes, classements — plus un widget OBS de résumé post-game.

Statut : **construit et en service** — parité fonctionnelle complète avec Stats of the Storm, patch
notes intégrés, overlays OBS, déployé via Docker. Pour le bundle tout-en-un auto-hébergé (un seul
`docker compose up`, référentiel pré-rempli), voir
**[storm-codex-suite](https://github.com/matella/storm-codex-suite)**.

## Documentation (commence ici)

| Doc | Quoi |
|---|---|
| [`docs/STATUS.md`](docs/STATUS.md) | Où en est le projet, la suite — à lire en premier |
| [`docs/specs/2026-06-12-storm-codex-design.md`](docs/specs/2026-06-12-storm-codex-design.md) | Le design validé du programme (architecture, modèle de données, budgets de perf, jalons) |
| [`docs/specs/2026-06-12-storm-codex-mockup.html`](docs/specs/2026-06-12-storm-codex-mockup.html) | Référence visuelle — les 14 écrans (à ouvrir dans un navigateur) |
| [`docs/research/2026-06-12-stats-of-the-storm-renovation.md`](docs/research/2026-06-12-stats-of-the-storm-renovation.md) | Dossier de recherche : anatomie de SotS, verdicts de dépendances, comparaison parser/engine, écosystème |
| [`docs/research/hots-replay-data-reference.md`](docs/research/hots-replay-data-reference.md) | Référence complète des emplacements de données `.StormReplay` (d'après la doc SotS) |

## Remerciements

Stats of the Storm et hots-parser par [@ebshimizu](https://github.com/ebshimizu) (MIT) — ce projet
est une ré-architecture de ces idées, pas un fork. Référence du format de replay grâce au même
projet. Heroes of the Storm™ est une marque de Blizzard Entertainment, Inc. Ce projet n'est pas
affilié à Blizzard.
