# API — référence REST + WebSocket

Routes définies dans `crates/storm-codex-server/src/main.rs` (source de vérité). Auth : seules
les routes marquées 🔒 exigent `Authorization: Bearer` (token d'upload nominatif pour l'upload ;
`ADMIN_TOKEN` pour l'admin — **si défini**, sinon mode ouvert).

## Upload

| Route | Rôle |
|---|---|
| `POST /api/upload` 🔒 | corps = octets bruts du `.StormReplay`, header `X-Filename` (percent-encodé accepté). Réponse ≤ 2 s : `parsed` / `409 duplicate` / `parse_failed` ; `202 accepted` si pool saturé (résultat par WS) |
| `POST /api/upload-raw` 🔒 | **alias strict** du précédent — compat client-rs (Hots-Overlay) |

## Lecture (`read.rs`)

| Route | Rôle |
|---|---|
| `GET /api/health` | `{status, parser_version, db}` — 200 ou 503 |
| `GET /api/matches` | liste paginée ; filtres `map`, `mode`, `hero`, `player`, `limit`/`offset` |
| `GET /api/matches.csv` | export CSV (mêmes filtres ; l'« export JSON » de l'UI = `/api/matches` avec `limit` élevé) |
| `GET /api/matches/{id}` | détail complet `{id, fingerprint, parser_version, match, players}` — `match` = objet storm-stats intégral (timeline, objectifs, `messages`…) |
| `GET /api/matches/{id}/raw?stream=…` | dump décodé à la volée (7 streams heroprotocol) + cache LRU |
| `GET /api/players/search?q=&limit=` | recherche de joueurs : `{hits:[{toon, name, matched_name, matched_old, games, wins, roles[2], last_seen}]}`. Cherche le pseudo actuel **et tous les anciens** (sous-chaîne, insensible à la casse, `_`/`%` littéraux) ; < 2 caractères → `hits: []` ; `limit` défaut 20, max 50 ; tri exact > préfixe > sous-chaîne puis parties. Route statique : prioritaire sur `{toon}` (axum). Mesuré sur le corpus dev (2 723 parties, 7 618 joueurs) : p95 26 ms (budget < 100 ms) |
| `GET /api/players/{toon}` | résumé joueur + hero pool ; `name` = pseudo actuel (historique daté), `names` = tous les pseudos vus, **récent d'abord** |
| `GET /api/players/{toon}/identity` | onglet Identité : `{toon, name, names:[{name, first_seen, last_seen, games}], suggestions:[{toon, name, level:"high"\|"low", score, reasons:[{kind, text}]}], confirmed:[{toon, name, decided_at}]}`. `suggestions` = **estimation** de comptes de la même personne (scoring pur `players/similarity.rs`, présélection bornée à 30 candidats `players/identity.rs`) ; exclut l'opérateur, les propriétaires de token d'upload (sauf signal token), les comptes déjà croisés dans une partie, les paires décidées. 404 si joueur inconnu. ~400 ms sur le corpus dev |
| `PUT /api/players/{toon}/links/{other}` 🔒 | corps `{status:"confirmed"\|"dismissed"}` : note « même personne » ou suggestion écartée (**aucune fusion de profils**). Paire normalisée (A→B ≡ B→A), idempotent ; 400 si `toon == other`, 404 si inconnu, 422 si statut invalide |
| `GET /api/heroes` · `GET /api/hero/{hero}` · `GET /api/hero/{hero}/patches` | agrégats héros ; croisement patch notes |
| `GET /api/hero-changes` · `GET /api/hero-changes/heroes` | sections héros des patch notes (buff/nerf) |
| `GET /api/synergies` | paires de héros (winrates ensemble/contre) |
| `GET /api/patches` · `GET /api/patches/{id}` | liste `dim_patches` ; détail (contenu) |
| `GET /api/maps` | agrégat par carte |
| `GET /api/dim/heroes` · `GET /api/dim/talents` | référentiels répliqués |
| `GET /api/dim/hero-attributes` | code attribut héros 4 lettres → nom canonique du parser (`heroAttribute` de `attr.json`, storm-stats) ; statique, sans DB. Résout les bans du draft, stockés bruts (`match.bans[t][i].hero` = `Crus`, `DEAT`…) — `dim_heroes` ne porte pas l'`attributeId` |
| `GET /api/trends` | winrate/durée par build/patch |
| `GET /api/now-playing` | proxy Orpheus (widget musique) |
| `GET /api/settings` | réglages applicatifs (dont `operator_names`) |

## Gestion (`manage.rs`, `admin.rs`) — 🔒 si `ADMIN_TOKEN` défini

| Route | Rôle |
|---|---|
| `PUT /api/admin/settings` | écrit `app_settings` (ex. `operator_names`) |
| `GET/POST /api/teams` · `PUT/DELETE /api/teams/{id}` | équipes (+ champ `league`) |
| `GET/POST /api/collections` · `DELETE /api/collections/{id}` | collections de matchs |
| `POST /api/admin/tokens` · `DELETE /api/admin/tokens/{id}` | tokens d'upload nominatifs (le clair n'est montré qu'à la création) |
| `GET /api/admin/uploads` | santé des uploads (statuts, classes d'échec) |
| `POST /api/admin/reprocess` | re-parse idempotent (piloté par `parser_version`) ; couvre aussi `scouting_games`, puis recalcule les rapports touchés |

## Scouting (`scouting/api.rs`) — écritures 🔒 si `ADMIN_TOKEN` défini

Spec : `docs/specs/2026-09-26-rapports-scouting-design.md`. Tables dédiées : ces routes n'écrivent
jamais dans `uploads`/`matches`/`match_players`. Toute mutation relance le recalcul des faits
(`facts_version + 1`).

| Route | Rôle |
|---|---|
| `GET /api/scouting` | liste (plus récent d'abord) : titre, équipe, dates, nb de parties, bilan, statut `empty`/`ready`/`analyzed`/`stale` |
| `POST /api/scouting` 🔒 | créer `{title, target_name?}` → `201 {id}` |
| `GET /api/scouting/{id}` | rapport complet : `snapshot` = `{facts, detection{candidates, ambiguous}, roster, roster_auto}`, `games` (équipes, côté cible + `target_source` roster/anchor/manual), `analysis` avec preuves résolues contre les faits courants + `analysis_tally`, `status` |
| `PATCH /api/scouting/{id}` 🔒 | `title`, `target_name`, `roster`, `anchors` (les deux derniers → recalcul) ; renvoie le rapport |
| `DELETE /api/scouting/{id}` 🔒 | supprime (parties en cascade, bruts archivés conservés) |
| `POST /api/scouting/{id}/replays` 🔒 | octets bruts + `X-Filename` → `added` (+ `gid`, côté détecté) / `duplicate` / `parse_failed` (+ `error_class`) |
| `PATCH /api/scouting/{id}/replays/{gid}` 🔒 | `{target_team: 0\|1\|null}` — côté manuel, `null` = retour à l'automatique |
| `DELETE /api/scouting/{id}/replays/{gid}` 🔒 | retire une partie |
| `GET /api/scouting/{id}/pack.md` | pack LLM Markdown (prompt, joueurs `pN`, faits avec ids, parties, format de réponse) |
| `GET /api/scouting/{id}/pack.xlsx` | mêmes faits en classeur (README, Facts, Players, Heroes, Maps, Draft, Games) |
| `PUT /api/scouting/{id}/analysis` 🔒 | corps = texte brut de la réponse du LLM ; extrait/valide/écrase → `{warnings, tally}` ; `422` si JSON introuvable/invalide, `format_version` ≠ 2 (plan de draft carte par carte ; la v1 est refusée) ou `report_id` d'un autre rapport (rien n'est écrasé) |
| `DELETE /api/scouting/{id}/analysis` 🔒 | retire l'analyse |

## Simulateur de draft (`draft/api.rs`)

`GET /api/draft` (état) · `POST /api/draft/config` · `/action` (pick/ban) · `/undo` · `/reset` ·
`/unavailable` (fearless) · `/score` · `/teams` (noms) · `/series/next` · `/series/new`.
Chaque mutation → WS `draft.updated` ; l'overlay et la console se re-fetchent.
**Série de cast active** (`cast.active`) : `/teams` et `/score` répondent `409 {"error":"series_active"}` (la série
est la source de vérité) ; `/config` reste accepté mais ignore `blue`/`red`/`bo` et force `format = série.format`
(map et first_pick restent libres). Série inactive : comportement inchangé.
`GET /api/draft` est la **vue publique** : en mode replay, sans script ni pseudo des picks non révélés.

**Draft depuis un replay** (`draft/replay_api.rs`, mode ouvert) — Custom uniquement, rien écrit en base :
`POST /api/draft/replay/load` (corps = octets `.StormReplay`, `X-Filename` optionnel ; décodé sous `parse_sem`, écriture du fichier temporaire, décodage et suppression **dans** le `spawn_blocking` avec garde de destruction — fichier supprimé même sur échec, panique ou client déconnecté, comme le replay de cast ; `201` file publique ; `500 {"error":"io"}` si le fichier temporaire ne peut être écrit ;
`400` corps vide ; `422` `{error: not_custom|no_draft|parse_failed}` ; `500` `{error: pool|io}` : sémaphore de parsing fermé / fichier temporaire non écrit) · `GET /api/draft/replay` (file
**avec scripts**, pour la page télécommande) · `POST …/replay/next` · `/back` · `/reset` (une **phase** à la fois ;
`409` sans partie / au début / à la fin) · `/select {index}` · `/fearless {value}` (série de cast active : `/select`,
`/fearless` et la sélection d'office de `/load` suivent le **format de la série**, pas `replay.fearless` — M5) ·
`/play {playing, interval_ms?}` (défaut 30 000 ms par phase ; `400` hors 10 ms–1 h ; la minuterie est côté
serveur, s'arrête à la fin du draft) · `POST …/replay/queue/move {from,to}` · `DELETE …/replay/queue/{i}`.
Spec : `docs/specs/2026-10-25-draft-replay-design.md`.

## Régie de cast (`cast/api.rs`, mode ouvert)

`GET /api/cast` (→ `Series`) · `POST /api/cast/series {blue:Team, red:Team, bo, format, pool:[nom]}`
(`blue`/`red` = équipe complète — nom, bilan, joueurs, capitaine, logo — tous champs optionnels, `{name}` seul accepté ; un `logo` absent garde le logo en place ; crée/redémarre en un seul appel : nouvelle identité `series_id` (uuid), `active=true`, parties/score/diaporama remis à zéro, scène conservée ; **draft aligné (R20)** : le moteur de draft reprend `format` et `steps` de la série, partie neuve (curseur 0) et nouvelle série (historique et series bans vidés), en gardant map, first_pick, prébans manuels, file de replays (conservée, mais partie courante désélectionnée et lecture auto stoppée, comme `/api/draft/config`) et équipes synchronisées (`DraftState::align_to_series`) ; `bo` ∈ {1,3,5,7}
sinon `400 {"error":"bad_bo"}`, état inchangé) ·
`PUT /api/cast/teams {blue, red}` · `POST /api/cast/pool/map {name, status: available|banned|picked|decider, by?}` ·
`POST /api/cast/games/start {map}` → `{number}` · `POST /api/cast/games/{n}/winner {side, takedowns?: [bleu, rouge], duration_secs?}` (**R21** : un vainqueur saisi à la main se **corrige** par un nouvel appel — score, mode de lineup et synchro draft suivent ; refusé `WinnerAlreadySet` seulement si la partie a un résumé de replay, qui porte son propre vainqueur/orientation) · `DELETE /api/cast/games/{n}` (**R21**, partie lancée par erreur : seulement la **dernière** — sinon `409 NotLastGame` — et sans vainqueur ni résumé — sinon `409 GameHasResult` ; la carte garde son statut picked/decider et redevient lançable, une partie épinglée dans le diaporama est désépinglée ; synchro draft) ·
`POST /api/cast/score {blue, red}` ou `null` (retour au score calculé) · `POST /api/cast/scene {scene: lineups|series|draft|ingame|hide}` ·
`POST /api/cast/lineup-mode {mode: auto|a|b}` · `POST /api/cast/slide {game?, index?, auto?, interval_ms?}`
(partiel ; `game: 0` désépingle la partie affichée → `slide.game` = null, « dernière partie » ; les numéros commencent à 1 ; **R25** : un autre `game` doit être le numéro d'une partie existante, sinon `409 UnknownGame` ; `index` ≤ 99 sinon `400 {"error":"bad_index"}` (vérifié **après** la garde `409 SeriesInactive` : une série inactive répond 409 même avec un `index` invalide ; l'overlay le prend modulo le nombre de slides) ; état inchangé sur refus ; `interval_ms` borné 500 ms–1 h, la régie saisit 1–3600 s). Les mutations répondent par la `Series` complète (`games/start` : `{number}`).
Erreurs `CastError` → `409 {"error": "<code>"}` — codes stables (`CastError::code()`, indépendants du nom Rust) :
`UnknownMap`, `NotInPool`, `DuplicateDecider`, `MapNotChosen`, `UnknownGame`, `WinnerAlreadySet`, `NotLastGame`,
`GameHasResult`, `MapAlreadyPlayed`, `SeriesInactive`, `side_mismatch`, `series_changed` — état inchangé.
**R24** : si l'écriture de la série en base échoue, la mutation répond `500 {"error":"persist_failed"}` ; l'état
reste appliqué en mémoire et `cast.updated` est diffusé (l'opérateur voit l'erreur dans le bandeau de la régie). Cas limite : si l'écriture v2 échoue puis qu'une sauvegarde v1 concurrente réussit, la base garde v1 alors que la mémoire garde v2 ; l'appelant de v2 a reçu `500 persist_failed` (cohérent avec R24 ; un redémarrage rechargerait v1).
Les écritures sont versionnées : un instantané plus ancien n'écrase jamais un plus récent (cf. 04-serveur).
Toute mutation sauf `/series` et `/scene` renvoie `409 SeriesInactive` tant que `active=false`.
`POST /api/cast/logo/{side}` (`blue`|`red` ; corps = octets, `Content-Type` png/jpeg/webp ; 1 octet–512 Ko ;
`415 unsupported_mime` type non accepté, `400` corps vide, `413` trop gros, `415 mime_mismatch` si les premiers octets ne
sont pas la signature du type déclaré — PNG `89 50 4E 47`, JPEG `FF D8 FF`, WebP `RIFF....WEBP` — rien n'est stocké ;
`404` côté inconnu ; autorisé série inactive) → `Series` avec
`Team.logo = "/api/cast/logo/{side}?v={updated_at_ms}"` · `GET /api/cast/logo/{side}` → octets (`404` si aucun ; `X-Content-Type-Options: nosniff`).
**Synchro série → draft** (sens unique) après `/series`, `/teams`, `/games/{n}/winner`, `DELETE /games/{n}`, `/score` : noms, pseudos
(`Player.name`), score (`Series::score()`) et `bo` copiés dans `DraftState` sans toucher curseur/assignations/file
replay/fearless ; les deux états sont persistés, WS `draft.updated` + `cast.updated`.
**Héros récents** (`cast/lineup.rs`, lecture seule, aucune mutation ni WS) : `GET /api/cast/lineup?blue=toon,toon,…&red=…`
→ `{ blue: [[HeroCount]], red: [[HeroCount]] }`, `HeroCount = {hero, games}` ; une liste par joueur **dans l'ordre reçu**
(toon vide = joueur non lié → `[]`). Par toon : fenêtre des **20 dernières parties tous modes** (`match_players` ⋈ `matches`,
tri `played_at DESC NULLS LAST, id DESC`), **3 héros** les plus joués (parties, puis récence) ; **`[]` si < 5 parties
dans la fenêtre** (masqué, jamais partiel) ; une partie au héros NULL compte dans ce seuil de 5 (partie jouée) mais pas dans le décompte des héros. Une seule requête SQL (`unnest … LATERAL`, index `match_players_toon_idx`,
vérifié par `EXPLAIN` dans le test : le plan doit nommer cet index, scan séquentiel désactivé sur la connexion ; lignes du test nettoyées même en cas d'échec) ; budget **< 100 ms pour 10 joueurs** (test `lineup_route_under_100ms_for_10_players`).
`> 10` toons par équipe → `400 {"error":"too_many_players"}` ; un toon de plus de 64 octets → `400 {"error":"bad_toon"}` ; erreur SQL → `500 {"error":"db"}`.
**Replay d'une partie** (`cast/replay.rs`) : `POST /api/cast/games/{n}/replay` — corps = octets bruts d'un
`.StormReplay` (comme `draft/replay/load`), décodé à la volée sous `parse_sem` (écriture du fichier temporaire, décodage et suppression **dans** le
`spawn_blocking` ; suppression garantie par un garde de destruction — succès, échec, panique, ou client déconnecté
pendant le décodage ; une panique du décodeur devient un 422). **Custom uniquement, rien n'est écrit en base** (ni `matches` ni `uploads` :
seule la ligne `cast_live`). → `201 GameSummary` (cf. [06-modele-donnees.md](06-modele-donnees.md)) ; remplit
`winner`, `takedowns`, `duration_secs` et `summary` de la partie `n`, **orienté sur la série** (R16, sous le
write-lock) : les joueurs du replay rattachés à ceux de la série (toon lié, puis pseudo exact) décident si l'équipe 0
du replay est la Blue ou la Red (`GameSummary::swapped()`) ; avec cette preuve le replay fait foi (remplace un
vainqueur saisi, ou un résumé précédent) ; **sans preuve**, équipe 0 = Blue, mais un vainqueur déjà saisi qui
contredit le replay → `409 {"error":"side_mismatch"}`, rien ne change. **`?swap=1`** : l'opérateur force l'échange.
Preuve = majorité simple des rattachements (égalité = pas de preuve) ; un joueur listé dans les deux équipes de la
série est ignoré. Persistance + synchro draft (score) + WS `cast.updated` comme les autres mutations ; la réponse est
le résumé orienté **plus** `orientation` ∈ `evidence` (preuve par les joueurs) | `assumed` (pas de preuve : équipe 0
= Blue) | `forced` (`?swap=1`), et `forced_against_evidence: true` quand l'échange forcé contredit une preuve
d'orientation directe (accepté, la régie l'affiche). Erreurs : `400 {"error":"empty_body"}` ; `409 SeriesInactive` / `409 UnknownGame` (partie `n` absente) /
`409 side_mismatch` / `409 series_changed` (l'identité `series_id` **ou la carte de la partie `n`** relevées avant le décodage ne correspondent plus : une nouvelle série a été démarrée, ou la partie `n` a été retirée (R21) puis relancée sur une autre carte pendant le décodage — rien ne change) ;
`422 {"error", "message"}` avec `error` ∈ `not_custom` (replay non Custom), `parse_failed` (octets illisibles,
tronqués, statut storm-stats ≠ 1), `incomplete` (vainqueur, durée ou takedowns absents de la sortie) — jamais 500
sur une entrée invalide ; `500 {"error":"pool"}` (sémaphore de parsing fermé) ou `500 {"error":"io"}` (fichier temporaire non écrit) sont des défaillances serveur. Budget décodage < 500 ms (mesuré : route complète 178–203 ms en release, upload local
compris, conditions non documentées — un run ultérieur sous charge a donné 417–444 ms, cf. `docs/STATUS.md` ; `storm-stats-dump --bench` sur 6 replays Custom : médiane 177 ms, max 319 ms).
Chaque mutation → WS `cast.updated`. Design : `docs/specs/2026-10-25-cast-control-room-design.md`.

## Feed live d'une partie en cours (`live/api.rs`)

Spec : `docs/specs/2026-10-24-feed-live-en-partie-design.md`. État **en mémoire**, une partie à la
fois, aucune table.

| Route | Rôle |
|---|---|
| `POST /api/live/chunk` 🔑 | octets ajoutés d'un stream, `Bearer` d'upload. En-têtes : `X-Session` (≤ 128 car. ; **SHA-256 hexadécimal du battlelobby** de la partie — seule façon pour le serveur de rapprocher la session du lobby courant et d'en tirer carte et opérateur ; un id nouveau **remplace** la session), `X-Stream` (`tracker` \| `game`), `X-Offset` (position du morceau dans le stream), `X-Build` (facultatif ; absent = dernier protocole embarqué). → `200 {status: "applied", consumed, last_loop, events}` ; `200 {status: "duplicate"}` (déjà reçu) ; `200 {status: "unreadable"}` (session illisible) ; `409 {status: "gap", expected_offset}` (renvoyer depuis là) ; `413 {status: "too_large"}` (stream > 64 Mo) ; `400` en-tête invalide ; `401` |
| `POST /api/live/end` 🔑 | `{session}` → statut `ended` ; `404` si ce n'est pas la session courante |
| `GET /api/live?scope=self\|allies\|all&me=<playerId>` | vue filtrée (défaut `scope=self`) ; `204` si aucune partie. Champs : `status` (`live`/`ended`/`unreadable` + `reason`), `session`, `map` et opérateur tirés du lobby **seulement si son hash = id de session**, `me` + `me_source` (`explicit`/`lobby`/`single_human`/`null` — indéterminé = aucune aptitude en `self`), `gates_loop`, `tracker_loop`, `game_loop`, `last_chunk_at`, `protocol {build, fallback, assumed_latest}`, `team_levels`, `players[]` (`player_id`, `team`, `hero`, `hero_unit`, `level`, `talents[{tree_id, loop}]`, `deaths`, `takedowns`, `abilities` = `null` si hors niveau, sinon `[{link, cmd, count, recent[loops des 3 dernières minutes]}]`), `feed[]` (200 plus récents : `death`/`talent`/`level`/`camp`/`objective`) |
| `DELETE /api/live` 🔒 | ferme la session (garde `ADMIN_TOKEN`, mode ouvert s'il n'est pas configuré) |

🔑 = jeton d'upload (comme `/api/upload-raw`). Chaque morceau qui ajoute des événements, et chaque
changement de statut → WS `live.updated` (signal seul).

## Statique

`/` = SPA (fallback `index.html` en `no-cache` — un redeploy change le hash du bundle) ;
`/images` = portraits héros + fonds de carte vendorisés ; `/assets` = bundle Vite fingerprinté.

## WebSocket `/ws`

Diffusion broadcast à tous les clients ; voir [07-evenements.md](07-evenements.md) pour les
types. Pas de messages entrants utiles (ping/close seulement). Un client en retard est
« laggé » sans déconnexion (events non critiques, re-fetch par TanStack Query).
