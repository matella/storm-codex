**Français** · [English](README.md)

# Storm Codex

Storm Codex transforme tes replays de Heroes of the Storm en un historique de parties que tu peux parcourir et analyser. Revois un combat sur la carte, compare deux joueurs ou regarde comment tes résultats varient selon le héros et le champ de bataille. L’application Windows conserve tes replays et tes statistiques sur ton PC, sans compte Storm Codex.

## Utiliser Storm Codex sous Windows

Télécharge `StormCodex_<version>_x64-setup.exe` depuis les [versions sur GitHub](https://github.com/matella/storm-codex/releases) lorsqu’une version publique est disponible. Les brouillons restent réservés aux responsables du projet. L’installateur embarque PostgreSQL et l’application le gère pour toi.

1. Lance Storm Codex. L’application cherche les dossiers de replays habituels du jeu et importe les fichiers `.StormReplay` des parties terminées.
2. Si tu conserves tes replays ailleurs, ajoute le dossier dans **Admin → This PC**. Vérifie **My identity** si l’application n’a pas reconnu ton compte.
3. Ouvre **Matches** pour consulter les parties importées. Laisse l’application ouverte pour qu’elle récupère les nouveaux replays après chaque partie.

La page d’une partie propose un replay 2D, les éliminations, les objectifs, les captures de camps et les combats détectés. Tu peux sélectionner un intervalle pour en revoir les événements, mais les dégâts et les soins ne sont disponibles que pour la partie entière.

## Auto-héberger avec Docker

Utilise [storm-codex-suite](https://github.com/matella/storm-codex-suite) pour la configuration Docker Compose. Le [guide d’exploitation](docs/spec/09-operations.md) décrit la configuration du serveur, les sources OBS et la publication de l’application Windows. L’application importe les replays des **parties terminées** ; elle ne fournit pas de flux pour une partie en cours.

## Code et documentation

- [`storm-replay`](crates/storm-replay) décode les fichiers `.StormReplay` à partir de tables générées avec [heroprotocol de Blizzard](https://github.com/Blizzard/heroprotocol).
- [`storm-stats`](crates/storm-stats) calcule les statistiques à partir des événements du replay.
- [`storm-codex-server`](crates/storm-codex-server) gère l’import des replays et l’API avec Axum et PostgreSQL.
- [`web-rs`](web-rs) contient l’application web Leptos et les sources OBS. [`desktop`](desktop) contient l’application Windows Tauri et la configuration de l’installateur.

Pour contribuer, consulte la [documentation technique](docs/spec/README.md), le [guide de développement](docs/spec/10-developpement.md) ou la [liste de vérification Windows](docs/standalone-acceptance.md). L’[état du projet](docs/STATUS.md) indique les travaux en cours.

## Aide et contributions

Consulte le [guide d’assistance](SUPPORT.md) pour signaler un bug, le [guide de contribution](CONTRIBUTING.md) pour proposer une modification et la [politique de sécurité](SECURITY.md) pour signaler une vulnérabilité en privé. Storm Codex est sous [licence MIT](LICENSE).

## Remerciements

Storm Codex est une réécriture en Rust inspirée de [Stats of the Storm](https://github.com/ebshimizu/stats-of-the-storm) et de [hots-parser](https://github.com/ebshimizu/hots-parser), créés par [@ebshimizu](https://github.com/ebshimizu). Heroes of the Storm est une marque de Blizzard Entertainment, Inc. Ce projet n’est pas affilié à Blizzard.
