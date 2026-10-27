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

## Guide de lecture — résumé de cast (`cast/summary.rs`)

Le post-game breakdown de la régie (`POST /api/cast/games/{n}/replay`) lit ces chemins, vérifiés sur
≈ 70 replays Custom réels (2026-08 → 10) et recoupés par un second chemin. **Aucune métrique
ajoutée à storm-stats** ; un chemin absent donne `None` / vide et l'écran retire la ligne.
Équipe 0 = Blue, 1 = Red. Les nombres peuvent être écrits en flottant (`28.0`).

| Valeur du résumé | Chemin storm-stats | Recoupement / remarque |
|---|---|---|
| `winner` (requis) | `match.winner` (0/1) | autre valeur → `incomplete` |
| `duration_secs` (requis) | `match.length`, tronqué | = `fmtDur` de la fiche (tronque) |
| `takedowns` (requis) | `match.team0Takedowns` / `team1Takedowns` | en-tête de la fiche ; **pas** `match.takedowns` (liste d'événements) ; ≠ somme des `SoloKill` sur quelques parties |
| `team_level` | `match.teams[t].level` | = max des clés de `match.levelTimes[t]` (source de la fiche) sur tout le corpus |
| `merc_camps` | `match.teams[t].stats.mercCaptures` | = nb de `match.mercs.captures` de l'équipe. La colonne avancée « Mercs » de la fiche somme `gameStats.MercCampCaptures` (participation, ≈ 2×) : **non utilisée** |
| `structures` (forts · keeps) | `teams[t].stats.structures.Fort.destroyed + .Keep.destroyed` | bâtiments seuls (sans tours ni puits) ; carte sans aucune sous-clé `Fort`/`Keep` (dans aucune équipe) ou `structures` absent → `None` (ligne retirée, jamais 0–0) ; sinon sous-clé absente = 0 ; = nb de `match.structures` Fort/Keep détruits chez l'adversaire |
| `objectives` | **liste blanche** par clé `MapType` (`rt_str("MapType", …)` comparé à `match.map`) ; événements **datés** seulement (`time` non nul, comme `objectiveEvents` de la fiche) | voir la liste ci-dessous ; toute autre carte → `None` (ligne retirée) |
| `bans` | `match.bans[t][].hero` (code attribut) → `attr.json` `heroAttribute` | code inconnu ignoré |
| `players[]` | clé de `players` (= `ToonHandle`) → `toon` ; `name`, `hero`, `team` ; `gameStats.{SoloKill→kills, Takedowns, Deaths, Assists, HeroDamage, SiegeDamage, Healing}` | `players[].deaths` / `.takedowns` (racine) sont des **listes** d'événements, pas des compteurs ; ne jamais dériver un compteur d'un autre (`Takedowns ≠ SoloKill + Assists` sur 7 lignes du corpus). Tri : Blue puis Red, ordre de `match.picks[t]` |
| `xp_lead` | `match.XPBreakdown[]` apparié par `loop` : Σ champs numériques de `breakdown` hors `*Time*` (comme la courbe « Team XP » de la fiche), Blue − Red, `(time tronqué, arrondi)` | présent sur 100 % du corpus Custom ; pas de repli `levelAdvTimeline` (unité différente : niveaux) |

**Liste blanche des objectifs (R17)** — chaque entrée : l'événement storm-stats (process.rs) qui
alimente `objective[t].events`, et le recoupement du corpus Custom :
- `TowersOfDoom` — `AltarCaptured` : un événement par autel capturé (count = events, 10–5, 4–9…).
- `CursedHollow` — `TributeCollected` : un événement par tribut ramassé par l'équipe (≤ `tributes` apparus : 3–0 sur 4, 4–8 sur 12).
- `DragonShire` — `DragonKnightActivated` : un événement par Chevalier dragon activé.
- `HauntedWoods` (Garden of Terror) — `GardenTerrorActivated` : une terreur activée par événement.
- `Crypts` (Tomb of the Spider Queen) — `SpidersSpawned` : une vague de Tisseuses (gemmes rendues) par événement.
- `Volskaya` — naissance du `Triglav` contrôlé par l'équipe : un protecteur capturé par événement.
- `BlackheartsBay` — `GhostShipCaptured` : un navire capturé par événement (corpus mince : une seule partie, en base, 3–2).
- `BattlefieldOfEternity` — `objective.results[].winner` (`ImmortalDefeated`) : immortel remporté.

**Exclues** (ligne retirée) : `ControlPoints` (Sky Temple, un événement par **tir** de temple, ≈ 241 par
partie) ; `AlteracPass` (une **unité** de cavalerie par événement) ; `Shrines` (Infernal Shrines :
`PunisherKilled` = punisseurs **morts** — sous-compte les autels gagnés, `shrines[].team` ≥ events sur
le corpus, ex. 3 vs 2) ; `Warhead Junction` (bombes **lancées**, pas réussies) ; `HauntedMines` (listes
de golems, pas d'événements par équipe) ; `BraxisHoldout`, `Hanamura` (pas d'événements par équipe) ;
cartes ARAM. Dans le doute, une carte reste hors liste.

**Orientation (R16, `Series::orient`)** : l'équipe 0 du replay n'est pas forcément la Blue de la
série. Les lignes `players[]` sont rattachées aux joueurs de la série (toon lié identique, puis pseudo
exact) : preuve nette d'un croisement → `GameSummary::swapped()` (paires et bans échangés, vainqueur
et côtés inversés, `xp_lead` négé) ; sans preuve, équipe 0 = Blue, sauf vainqueur saisi contraire →
`409 side_mismatch`. `?swap=1` force l'échange.

Comparatifs d'équipe affichés (K/D/A, dégâts héros, dégâts de siège) = sommes des lignes joueurs,
comme la ligne « Team total » de la fiche.

## Performance

133 ms médiane parse complet (decode + stats, mono-thread, `fast-alloc`) — budget < 150 ms.
Bench : `storm-stats-dump --bench <dir>` (sépare parse complet et rejets).
