# storm-stats

Stats de match **Heroes of the Storm** depuis un replay décodé (`storm-replay`) — port Rust
fidèle de [hots-parser](https://github.com/ebshimizu/hots-parser).

```rust
let out = storm_stats::process_replay(std::path::Path::new("match.StormReplay"), "match.StormReplay");
if out.status == 1 {
    let json = out.to_json(); // forme { match, players } de hots-parser
}
```

## Conception

- **Port 1:1 de `parser.js`** (3 360 lignes), bugs compris : la sortie reproduit exactement
  `{match, players, status}` de hots-parser, y compris ses statuts d'échec. Tout écart volontaire
  est une **tolérance documentée**, jamais silencieuse.
- **Parité prouvée, pas déclarée** : `tools/parity-harness/` exécute hots-parser (Node) et
  storm-stats sur le même corpus et diff **champ par champ**.
  → **114/114 replays verts** (79 au parse complet identique, 35 rejetés identiquement).
  Détails : `docs/research/2026-06-12-jalon2-parite.md`.
- **Constantes embarquées** (`data/constants.json`, `data/attr.json`) : exportées de
  hots-parser, régénérables.

## Périmètre (parité totale SotS)
Identité match/joueurs · score screen (~80 stats/joueur) + awards · talents par palier · draft
(picks/bans ordonnés, first pick) · takedowns enrichis (positions, participants, vengeances) ·
objectifs **des 16 cartes** (immortels, dragon, araignées, tributs, autels, crânes, navires,
punishers, Braxis avec force de vague, temples, trônes, mines, graines, gemmes, protecteur) ·
mercs/structures · XP périodique + level advantage · team fights/uptime · taunts/BM
(bsteps, danses, sprays, voicelines) · messages/pings · votes · globes · stats d'équipe.

## Tolérance documentée
`match.messages.*.point.x/y` (coordonnées de ping) : hots-parser utilise le port heroprotocol
de GaryIrick qui décode mal ce champ (overflow signé) ; storm-stats suit le port Blizzard et
donne la valeur correcte. Voir `tools/parity-harness/tolerances.json`.

## Performance
Parse complet (decode + stats), Ryzen 7 7800X3D mono-thread, feature `fast-alloc` :
**133 ms médiane** sur échantillon représentatif (budget spec : < 150 ms), dominé par le
décodage. `storm-stats-dump --bench <dir>` pour reproduire.

## Licence
MIT. Logique dérivée de hots-parser (@ebshimizu, MIT). Non affilié à Blizzard.
