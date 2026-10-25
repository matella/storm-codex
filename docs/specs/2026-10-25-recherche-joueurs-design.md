# Recherche de joueurs + comptes liés (smurfs) — design

> Statut : **validée par l'opérateur le 2026-10-25, amendée le même jour** (voir « Amendements »).
> Issu d'un brainstorming du 2026-10-25. Les points marqués **[mon choix]** sont des
> décisions prises par défaut, pas validées : à relire en priorité.

## Intention

L'opérateur veut retrouver un joueur précis et savoir ce qu'il joue et ce qu'il fait de bien. Sur HotS
beaucoup de joueurs ont des smurfs : il veut aussi pouvoir repérer qu'un compte « ressemble » à un autre.

Un smurf est un autre compte Blizzard (autre `toon_handle`) : **aucun replay ne relie deux comptes
directement**. Le lien ne peut être qu'une estimation probabiliste, toujours présentée comme telle.

## Objectifs

1. **Recherche** : une boîte de recherche trouve un joueur par pseudo actuel, **n'importe quel ancien
   pseudo**, ou fragment de pseudo. Un joueur peut avoir une longue liste de renommages.
2. **Profil joueur** : héros, rôles, winrate, builds/talents forts, cartes, parties récentes.
3. **Comptes liés** : onglet « Identité » avec l'historique daté des pseudos et une liste devinée
   d'autres comptes, chacun avec un score et ses raisons.
4. **Confirmer / écarter** un lien deviné ; le choix est mémorisé.

## Hors périmètre / anti-objectifs

- **Aucune fusion de profils.** Un lien confirmé est une simple note « même personne » : chaque compte
  garde ses propres stats. Pas de winrate ou hero pool fusionné.
- Jamais présenter une supposition comme un fait ; pas de score sans raisons lisibles.
- Aucun joueur absent de nos replays : la couverture = les replays uploadés. Pas d'appel à une API
  Blizzard externe.
- Pas de lien inféré entre un compte et une personne réelle (pas d'identité civile).

## Comportement

### Recherche (`/players`)
- Une boîte ; correspondance insensible à la casse et aux accents, par sous-chaîne, sur le pseudo
  actuel **et** tous les anciens pseudos. Résultat : pseudo qui a matché (marqué « was … » si ancien),
  nb de parties, winrate, rôles principaux, dernière partie vue. Tri : exact > préfixe > sous-chaîne,
  puis par nb de parties.
- Vide / trop court (< 2 caractères) : pas de requête.

### Profil (`/players/:toon`)
- Pseudo actuel, **3 anciens pseudos les plus récents** + « tout voir (N) » qui ouvre la liste complète
  (liste potentiellement très longue : repliée par défaut, défilante, jamais tronquée en silence).
- Si un compte lié **de confiance haute** existe (ou un lien confirmé) : une ligne « Joue aussi sous
  X → » vers l'onglet Identité. Les liens de confiance basse ne s'affichent pas ici.
- Onglets : Stats · Héros · Parties · Identité.

### Onglet Identité
- Historique des pseudos, **daté** : « StormyNight, vu jan–août 2025 ».
- Comptes liés devinés : pseudo, niveau (HAUT/BAS) + score, raisons détaillées (voir signaux).
- Actions par suggestion : **Confirmer** (note « même personne ») · **Pas la même personne** (écarte et
  n'est plus reproposé). Liens confirmés listés à part.

## Signaux de similarité (compte A ↔ compte B)

Données disponibles aujourd'hui (vérifié dans le code) : `match_players` (hero, team, talents, win),
`matches.played_at`, `players.names` (liste plate, **sans dates**), `uploads.token_id` (uploader).
**Pas d'information de groupe/premade dans les données** : « mêmes amis » est approximé par des
co-équipiers récurrents.

| Signal | Force | Note |
|---|---|---|
| **Propriétaire du même token** : deux comptes présents chacun dans ≥ 50 % des parties d'un même token (≥ 50 parties) | forte | « même uploader » brut est inopérant : chaque replay contient les 10 joueurs. Ne prouve pas à lui seul (prêt de PC) |
| Co-équipiers récurrents communs | moyenne | approximation du groupe d'amis ; **hors opérateur (`operator_names`) et propriétaires de token**, sinon tout le monde est lié via eux |
| Chevauchement du pool de héros | faible-moyenne | seul, ne suffit jamais pour « HAUT » |
| Habitudes de talents sur mêmes héros | moyenne | |
| Plages horaires de jeu | faible | jamais seul |
| Exclusion temporelle | négatif | deux comptes jouant **en même temps** dans la même partie ou sur des parties qui se chevauchent ≠ même personne |

Niveau HAUT : exige au moins **deux signaux indépendants** dont un « fort ou moyen ». Les poids et
seuils exacts sont laissés au plan d'implémentation, à régler sur des cas réels (pas de valeur
arbitraire figée ici).

## Amendements (2026-10-25)

1. Signal « même uploader » redéfini en « propriétaire de token » ; opérateur et propriétaires de token exclus des co-équipiers et de la présélection (constat sur le corpus dev : un token = 2 719 des 2 723 parties).
2. **Pseudo actuel, « was … » et « 3 plus récents » viennent de l'historique daté** (`match_players` + `matches.played_at`, NULL = plus ancien), pas de `players.last_name` (écrasé dans l'ordre de traitement). `players.names` ne sert que d'ensemble pour le matching.
3. Routes front : `player/:toon` (existante) conservée ; `/players` = recherche. Remplace `/players/:toon`.
4. Pas de `CREATE EXTENSION` : recherche en `lower()` + `ILIKE` ; trigram seulement si la mesure l'exige, via migration séparée validée. Insensibilité aux accents hors v1.
5. Couverture : un joueur vu **uniquement** dans un rapport de scouting (tables isolées, migration 0010) n'est pas cherchable.
6. Le scoring est une fonction pure : `crates/storm-codex-server/src/players/similarity.rs`.

## Données

- `players.names` (JSONB plate) est conservée telle quelle. L'**historique daté** se dérive de
  `match_players` + `matches.played_at` (premier/dernier jour vu par pseudo) — dérivable, donc pas de
  donnée stockée supplémentaire à re-process.
- Nouvelle table pour les décisions : `player_links (toon_a, toon_b, status ∈ confirmed/dismissed,
  decided_at)`, paire stockée ordonnée. Les suggestions elles-mêmes sont **calculées**, pas stockées.
- Une migration (`0011_player_links.sql`), `parser_version` non concerné. Pas d'extension (voir Amendements).

## API (à documenter dans `docs/spec/05-api.md` à l'implémentation)

- `GET /api/players/search?q=` — résultats de recherche.
- `GET /api/players/{toon}` — existant ; `name`/`names` viennent de l'historique daté.
- `GET /api/players/{toon}/identity` — historique daté + comptes liés devinés + liens décidés.
- `PUT /api/players/{toon}/links/{other}` 🔒 `{status}` — confirmer / écarter.

## Visibilité et droits

- **Tout le monde voit** l'onglet Identité et les notes « même personne » (décision de l'opérateur).
- **[mon choix]** Confirmer/écarter est une écriture admin : gardée par `ADMIN_TOKEN` comme les autres
  écritures (mode ouvert s'il n'est pas défini), pour qu'un visiteur ne puisse pas réécrire les liens.
- Risque assumé : les suppositions sont visibles de tous ; d'où l'étiquetage permanent « estimation »
  et l'absence de lien affiché en confiance basse sur le profil principal.

## Contraintes

- Budget perf : `GET /api/players/search` p95 < 100 ms (budget API de la spec programme) ; le calcul
  des comptes liés peut être plus lent mais doit rester borné (candidats présélectionnés, pas de
  comparaison de tous les joueurs deux à deux) ; à mesurer sur le corpus réel.
- Front : React + TanStack, tokens Nexus Codex (`ds.css` / `theme.css`), maquette validée visuellement
  (`.superpowers/brainstorm/…/search-and-profile-nexus.html`).
- Doc : mise à jour de `docs/spec/05-api.md`, `06-modele-donnees.md`, `08-frontend.md` dans le même
  commit que le code (règle n° 0).

## Laissé au constructeur

- Poids/seuils des signaux ; algorithme de présélection des candidats ; trigram vs `ILIKE` ;
  composants exacts ; format des raisons affichées.

## Questions ouvertes pour l'opérateur

1. Valides-tu que **confirmer/écarter** soit réservé à l'admin (`ADMIN_TOKEN`) ?
2. La longueur de l'historique « vu jan–août » dépend des replays : acceptable que les renommages
   antérieurs aux replays soient invisibles ?
