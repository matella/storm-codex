# storm-replay

Décodeur Rust de replays **Heroes of the Storm** (`.StormReplay`) : archive MPQ + les
7 streams du protocole Blizzard (header, details, initdata, attributes, messages,
game events, tracker events).

```rust
let replay = storm_replay::Replay::open("match.StormReplay")?;
println!("build {} — {}", replay.header.base_build, replay.details()?.title);
for event in replay.tracker_events()? {
    // Value typé : structs/blobs/ints fidèles au protocole
}
```

## Conception

- **Tables de protocole générées, pas écrites à la main** : `tools/protocol_gen.py` exporte les
  `typeinfos` de chaque build depuis un clone GitHub de
  [Blizzard/heroprotocol](https://github.com/Blizzard/heroprotocol) (jamais le package PyPI,
  obsolète et cassé sur Python ≥ 3.12), les déduplique (390 builds → 32 tables, 0,6 MB) et les
  embarque dans le crate (`protocols/`). À relancer à chaque patch HotS.
- **Builds inconnus** : fallback sur le dernier protocole connu (comportement heroprotocol),
  signalé par `Replay::protocol_fallback()` — jamais silencieux.
- **Décodage paresseux par stream** : `Replay::open` ne décode que le header ; chaque stream est
  décodé à la demande.
- **Parité bit-exacte prouvée** : `tools/crosscheck_streams.py` deep-compare la sortie des
  7 streams contre heroprotocol (Python) — égalité champ par champ, y compris ~100 000 game
  events par replay.
- Erreurs typées (`thiserror`), classes stables pensées pour les stats d'échec du serveur.

## Performances (Ryzen 7 7800X3D, mono-thread, feature `fast-alloc`)

| Mesure | Valeur |
|---|---|
| 7 streams complets, médiane (corpus 50 replays réels) | **102 ms** |
| 7 streams complets, p95 | 205 ms (dont ~50–115 ms de plancher bzip2 incompressible) |
| Hot path stats (header + details + tracker) | ~12 ms |
| Archive réelle (2 821 replays, 22 builds 2024→2026) | **100 % décodés** |

La feature `fast-alloc` (mimalloc, utilisée par les binaires) divise le temps de décodage par
~1,6 sur Windows — recommandée pour tout consommateur intensif.

## Binaires

- `storm-replay-dump <replay> --stream <nom>` — dump JSON d'un stream (normalisé heroprotocol) ;
  `--bench <dir>` — bench de décodage complet.
- `storm-replay-verify <dir> [--csv out.csv]` — décode récursivement une archive entière,
  classe les échecs par type d'erreur.

## Licence

MIT. Référence de format : heroprotocol (Blizzard, MIT). Heroes of the Storm™ est une marque de
Blizzard Entertainment ; ce projet n'est pas affilié à Blizzard.
