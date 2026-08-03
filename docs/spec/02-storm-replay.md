# storm-replay — contrat du décodeur

Crate : `crates/storm-replay` (bibliothèque publique, MIT). Vitrine : son `README.md`.

## Contrat

- **Entrée** : un fichier `.StormReplay` (archive MPQ Blizzard).
- **Sortie** : les **7 streams** du protocole, en `serde_json::Value` normalisé **identique à
  heroprotocol** (parité bit-exacte prouvée par `tools/crosscheck_streams.py`) :
  `header`, `replay.details`, `replay.initdata`, `replay.attributes.events`,
  `replay.message.events`, `replay.game.events`, `replay.tracker.events`.
- **Décodage paresseux** : `Replay::open` ne décode que le header ; chaque stream à la demande
  (`details()`, `tracker_events()`, `message_events()`, …).
- **Erreurs typées** (`thiserror`), classes stables — le serveur s'en sert pour classer les
  échecs d'upload (`error_class`).

## Tables de protocole

- **Générées, jamais écrites à la main** : `tools/protocol_gen.py` exporte les typeinfos depuis
  un clone GitHub de `Blizzard/heroprotocol` (pas le package PyPI, obsolète), déduplique
  (390 builds → ~32 tables) et les embarque dans `protocols/` (committées).
- **Build inconnu** → fallback sur le dernier protocole connu (comportement heroprotocol),
  signalé par `Replay::protocol_fallback()` — jamais silencieux.
- **À chaque patch HotS** : relancer `protocol_gen.py` si le build introduit un nouveau protocole.

## Performance

Médiane 102 ms les 7 streams (p95 205 ms, plancher bzip2 ~50–115 ms incompressible) ; hot path
stats (header+details+tracker) ~12 ms. Feature `fast-alloc` (mimalloc) recommandée pour tout
consommateur intensif (~×1,6 sur Windows). Archive réelle : 2 821/2 821 décodés (22 builds
2024→2026).

## Binaires de diagnostic

- `storm-replay-dump <replay> --stream <nom>` — dump JSON d'un stream ; `--bench <dir>`.
- `storm-replay-verify <dir> [--csv]` — décode une archive entière, classe les échecs.

## Tests

`tests/mini_corpus.rs` sur les replays committés de `tests/data/` (CI publique sans corpus
privé). Le corpus complet vit sur le NAS/box (`fetch-corpus`).
