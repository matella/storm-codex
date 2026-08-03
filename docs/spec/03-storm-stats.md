# storm-stats — contrat des stats & parité

Crate : `crates/storm-stats` (bibliothèque publique, MIT). Port Rust **1:1** de
[hots-parser](https://github.com/ebshimizu/hots-parser) (`parser.js`, 3 360 lignes), bugs compris.

## Contrat

- **Entrée** : chemin d'un `.StormReplay` + nom de fichier (utilisé par certaines heuristiques).
- **Sortie** : `Output { match_, players, status }` → `to_json()` reproduit exactement la forme
  `{match, players, status}` de hots-parser. `status == 1` = parse complet ; les autres statuts
  reproduisent les rejets de la référence (cf. `reject_class` côté serveur).
- **Périmètre** (parité SotS totale) : identité match/joueurs · score screen (~80 stats/joueur) +
  awards · talents par palier · draft ordonné (picks/bans, first pick) · takedowns enrichis ·
  objectifs des 16 cartes · mercs/structures · XP périodique + level advantage · team fights ·
  taunts/BM · **messages/pings/annonces** (`match.messages`) · votes · globes · stats d'équipe.

## Parité : la règle

**Prouvée, pas déclarée.** Le harnais `tools/parity-harness/` (`dump.js` côté Node, `diff.py`,
`tolerances.json`) diffe champ par champ contre hots-parser 7.55.7 sur le corpus de référence :
**114/114 verts** (79 parse complet identique, 25 extension ARAM, 10 rejets brawls identiques).
Toute modification de `process.rs` re-passe le diff avant merge.

## Divergences assumées (les SEULES — toute nouvelle divergence se documente ici + tolerances.json)

1. **Coordonnées de ping** (`match.messages[].point.x/y`) : hots-parser décode via le port
   GaryIrick (overflow signé) ; storm-stats suit le décodeur Blizzard — valeur correcte.
2. **Cartes ARAM récentes** (`EXTRA_MAPS` : Silver City, Lost Cavern, Braxis Outpost, Industrial
   District) : rejetées par la `MapType` de hots-parser 7.55.7 (~30 % de l'archive). storm-stats
   les parse (objectif minimal, chemins universels sinon). Validation par invariant structurel
   (`tests/extended_maps.rs`), pas de baseline Node possible.

## Constantes embarquées

`data/constants.json` + `data/attr.json`, exportées de hots-parser (régénérables). Y vivent
notamment `MessageType` (Chat 0 / Ping 1 / LoadingProgress 2 / PlayerAnnounce 5),
`MessageTarget` (All 0 / Allies 1 / Observers 4), `GameMode`, `MapType`, `ScoreEventNames`…
Le front réplique les valeurs qu'il affiche (cf. [08-frontend.md](08-frontend.md)).

## Performance

133 ms médiane parse complet (decode + stats, mono-thread, `fast-alloc`) — budget < 150 ms.
Bench : `storm-stats-dump --bench <dir>` (sépare parse complet et rejets).
