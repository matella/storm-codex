# Chat de match — refonte

> Spec storm-codex. Scope retenu par l'opérateur le 2026-09-08 (les quatre volets).
> **En attente de validation** avant `writing-plans` et implémentation.

## But

Le fil de chat existe (`ChatLog`, `web/src/pages/MatchDetail.tsx:269`) mais reste une liste
passive : on lit qui a dit quoi, sans jamais savoir **où en était la partie**. Or les données pour
répondre à ça sont déjà décodées et déjà servies — elles ne sont simplement pas exploitées. La
refonte relie le chat au temps de jeu, le rend cherchable, et étend la recherche à toute l'archive.

## L'existant (état au 2026-09-08)

| Élément | Où | Ce qu'il fait |
|---|---|---|
| Extraction des messages | `crates/storm-stats/src/process.rs:1905` (`process_messages_and_bm`) | `type`, `player`, `team`, `recipient`, `loop`, `time`, puis `text` / `point{x,y}` / `announcement` |
| Transport | `GET /api/matches/{id}` → `match.messages` | rien de spécifique au chat, tout vient de l'objet match |
| Affichage | `ChatLog` | horloge, avatar, nom coloré, badge `all`/`allies`/`obs`, texte ; bascules pings et callouts |
| Comptage des pings par joueur | `BMTable` (`MatchDetail.tsx:239`) | colonne « Pings » |

**Ce que les données portent et que rien n'affiche** : `point{x,y}` des pings
(`process.rs:1953`), alors qu'une visionneuse 2D avec les vraies minimaps existe dans l'onglet
d'à côté.

**Limite connue, conservée** : les messages des **observateurs sont écartés** par le parser (filtre
`player_lobby_id`, `process.rs:1936`), conformément à hots-parser. Sans effet sur les modes
matchmakés ; visible en partie personnalisée. On ne la lève pas ici — ce serait une divergence de
parité à instruire dans `03-storm-stats.md`, hors sujet de cette spec.

## Décisions actées

1. **Aucun changement de la forme de la projection ni de `PARSER_VERSION`** pour les volets A à C :
   tout est déjà dans `match.messages`. Le volet D ajoute une **table de lecture**, pas un champ.
2. Le chat reste **dans l'onglet score**, la visionneuse dans le sien : on les relie, on ne les
   fusionne pas.
3. Les helpers de filtrage sont des **fonctions pures dans `web/src/api.ts`**, testées en vitest
   (convention de `10-developpement.md` : helper pur → test vitest, pas de doc de contrat à bouger).
4. Le volet C ne doit **pas alourdir le chargement de la fiche de match** : les données de contexte
   viennent de `/api/matches/{id}/replay2d`, fetchées **à la demande** quand l'opérateur active le
   contexte, et partagées avec l'onglet visionneuse par le cache TanStack (`["replay2d", id]`,
   `staleTime: Infinity`).

## Volet A — Chat ↔ visionneuse 2D

### A1. Cliquer un message positionne la visionneuse

Cliquer l'horodatage d'un message bascule sur l'onglet `replay2d`, positionne la lecture à
`msg.time` et met en pause. Retour d'un clic sur l'onglet score, le fil ayant conservé sa position.

Implémentation : `Replay2D` possède aujourd'hui son `t` en état local (`Replay2D.tsx:229`) et n'est
monté qu'à un seul endroit (`MatchDetail.tsx:357`). On lui ajoute deux props **optionnelles**
(`t?`, `onSeek?`) — non fournies, le composant se comporte exactement comme aujourd'hui ; fournies,
`MatchDetail` détient l'état. Pas de refonte de `usePlayback`, qui ne possède déjà pas `t`.

### A2. Pings placés sur la minimap

Les pings s'affichent à leur position réelle pendant une fenêtre de quelques secondes autour de
`t`, comme les morts (`deathsNear`, `web/src/replay2d.ts:71`).

**Point à vérifier avant de s'y engager** : le repère de `m_point` n'est pas prouvé identique à
celui des positions de héros. La visionneuse normalise par `x / MapSizeX` avec des coordonnées en
tuiles ×4096 (`crates/storm-replay-viewer/src/extract.rs:16-21`), et `process.rs:1950` documente
déjà une divergence d'interprétation **signée** avec hots-parser sur ce champ précis. La première
tâche du volet est donc une **vérification** : superposer les pings d'un replay connu et confirmer
qu'ils tombent sur la carte, dans les bonnes zones (un ping « danger » près d'un camp, pas hors
cadre). Si le repère ne correspond pas, A2 est **abandonné** et A1 livré seul — pas de position
approximative affichée comme une vérité.

## Volet B — Recherche et filtres

Purement front, aucun changement d'API.

- **Champ de recherche** sur le texte (insensible à la casse et aux accents).
- **Chips par joueur** (les 10 du match, avec avatar) — cumulatives.
- **Filtre par destinataire** : `all` / `allies` / `obs` (la valeur est déjà affichée, pas encore
  filtrable).
- Les bascules pings / callouts existantes sont conservées et rejoignent la même barre.
- Un compteur « N / M messages » indique en permanence ce que le filtre masque.

Helper pur `filterMessages(messages, {q, players, targets, types})` dans `api.ts`, testé en vitest :
casse, accents, filtre vide = tout, cumul joueur × cible, message sans texte.

## Volet C — Contexte de jeu dans le fil

Trois ajouts, tous alimentés par `/api/matches/{id}/replay2d`, activés par une bascule
« context » (repliée par défaut) :

1. **Message envoyé en étant mort** : marque discrète sur la ligne. Source exacte —
   `heroes[].life: Interval[]` (`web/src/replay2d.ts:9`), pas une heuristique de proximité de mort.
2. **Événements intercalés** dans le fil : morts (`deaths[]`) et objectifs (`objectives[]`), en
   lignes de séparation typographiquement distinctes des messages. C'est ce qui donne son sens au
   fil (« il flame après un triple kill »).
3. **Densité de chat par minute** : une bande alignée sur l'axe temps du graphe de niveau existant
   (`LevelChart`), qui sert aussi de navigation — cliquer une minute fait défiler le fil.

Non-but : aucune analyse de ton, de toxicité ou de sentiment. Le fil montre, il ne juge pas.

## Volet D — Recherche de chat inter-matchs

Chercher une phrase dans toute l'archive (~3 300 matchs). C'est le seul volet qui touche la base et
l'API.

### Modèle de données — migration `0010_match_messages.sql`

Table de **lecture** dérivée, pas une nouvelle source de vérité :

```sql
CREATE TABLE match_messages (
    match_id    BIGINT NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    seq         INT    NOT NULL,          -- index dans match.data->'messages'
    toon_handle TEXT,
    team        INT,
    recipient   INT,
    loop        INT,
    time_s      DOUBLE PRECISION,
    text        TEXT   NOT NULL,
    PRIMARY KEY (match_id, seq)
);
CREATE INDEX match_messages_trgm_idx ON match_messages USING GIN (lower(text) gin_trgm_ops);
```

- **Chat seulement** (`type = 0`) : pings et callouts n'ont pas de texte à chercher.
- **Trigrammes plutôt que `tsvector`** : le chat est court, bilingue, mal orthographié et plein de
  pseudos — la recherche de sous-chaîne tolérante sert mieux que la lemmatisation, et évite d'avoir
  à choisir une configuration de langue. Nécessite l'extension `pg_trgm` (créée par la migration).
- **Backfill dans la migration elle-même**, par `jsonb_to_recordset` sur `matches.data->'messages'` :
  aucun reprocess des 3 300 replays n'est nécessaire, la donnée est déjà en base.
- `project_match` alimente la table à chaque projection ; l'idempotence suit celle des matchs (le
  `DELETE FROM matches` en cascade, `project.rs:104`).

### API — `GET /api/chat`

`q` (obligatoire, ≥ 2 caractères), `player`, `mode`, `from`/`to`, `limit`/`offset`. Renvoie les
messages avec leur match, l'auteur résolu et le contexte immédiat (message précédent et suivant),
triés par date de partie décroissante. **Budget : p95 < 100 ms** (contrat d'API du projet), mesuré
sur l'archive réelle.

### Front — page `/chat`

Résultats groupés par match, extrait surligné, clic → `/match/:id` **ancré sur le message**
(`#msg-<loop>`, ancre également posée par le volet B pour le partage d'un message précis).

## Ordre de livraison

B (front pur, zéro risque) → A1 → A2 *si la vérification du repère passe* → C → D. Chaque volet est
livrable et vérifiable seul.

## Critères d'acceptation

| Volet | Critère mesurable |
|---|---|
| A1 | Sur un replay réel, cliquer 3 messages à des temps distincts positionne la visionneuse à ±0,5 s ; vérifié dans le navigateur |
| A2 | Les pings d'un replay réel tombent dans le cadre de la carte et sur des zones plausibles — sinon le volet est abandonné et la raison écrite ici |
| B | `npm test --prefix web` vert sur `filterMessages` ; recherche « gg » sur un match réel ne renvoie que des lignes contenant « gg » |
| C | Un message dont l'auteur est mort est marqué, et l'intervalle `life` correspondant le confirme dans les données ; le fil reste fluide sur le plus long replay du corpus |
| D | Migration appliquée sur le Postgres dev, **backfill vérifié** (`SELECT count(*)` cohérent avec la somme des messages de type 0 de `matches.data`) ; `GET /api/chat?q=…` mesuré p95 < 100 ms sur l'archive |

## Impact documentaire (règle n° 0)

| Fichier | Ce qui change |
|---|---|
| `docs/spec/08-frontend.md` | ligne `/match/:id` (chat relié à la visionneuse, filtres, contexte) + nouvelle route `/chat` |
| `docs/spec/05-api.md` | `GET /api/chat` |
| `docs/spec/06-modele-donnees.md` | table `match_messages`, extension `pg_trgm`, migration `0010` |
| `docs/spec/01-architecture.md` | p95 mesuré de `/api/chat` |

## Question ouverte pour l'opérateur

Les messages d'observateurs restent écartés (parité hots-parser). Tu joues des parties
personnalisées avec observateurs — veux-tu qu'on instruise la levée de ce filtre dans une spec
séparée, ou ça reste sans intérêt pour toi ?
