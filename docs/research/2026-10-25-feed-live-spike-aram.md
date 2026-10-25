# Spike feed live — une partie matchmakée à 10 humains (2026-10-25)

> Spike exigé par `docs/specs/2026-10-24-feed-live-en-partie-design.md` (section « Spike », critères
> 1 et 3) avant de déclarer la feature finie. Suite de `docs/research/2026-10-24-tempwritereplay-live.md`
> (une partie Sandbox, un seul humain). **Une seule partie observée**, en **ARAM** (pas QM ni Storm
> League) : 10 humains, matchmakée, mais au rythme d'action propre à l'ARAM.

## La partie

- Replay final : `2026-10-25 14.32.20 Braxis Outpost.StormReplay` (863 735 octets), mode `50101`
  (ARAM), build **98348**, 16 511 loops, défaite côté opérateur (`matella#21409`, Kel'Thuzad, joueur 9).
- Héros : Anduin, Li-Ming, Zagara, Genji, Stitches (équipe 0) · Arthas, Rehgar, Azmodan, Kel'Thuzad,
  Kerrigan (équipe 1).

## Montage

- **Watcher réel** : `client-rs` (repo Hots-Overlay, `src/live.rs`), binaire release, mode headless,
  sur le vrai `%TEMP%\Heroes of the Storm\TempWriteReplayP1\`. Il n'envoie, en cours de partie, que
  jusqu'au dernier multiple de 4 Ko de chaque stream, puis la queue au flush final.
- **Serveur** : `storm-codex-server` release (même binaire que `f2feb90`) en local sur `:8091`, Postgres
  dev. Le box n'était pas en jeu : les retards mesurés viennent de l'écriture du jeu, pas du serveur.
- **Enregistreur** (script jetable, hors repo) : toutes les 2 s, `GET /api/live?scope=all` + taille
  de chaque fichier temporaire (ouverture partagée) + chemin du process `HeroesOfTheStorm_x64` ; copie
  du dossier toutes les 30 s. Le journal du watcher (`client.log`, lignes `live:`) donne chaque bloc
  envoyé avec son heure et le `last_loop` répondu par le serveur.

## Point 1 — les ordres des 10 joueurs sont-ils là en cours de partie ? **Oui.**

- Le `game.events` temporaire contient les ordres des **10** joueurs pendant la partie : la vue `all`
  montre des pressions d'aptitudes pour les joueurs 1 à 10, toutes apparues sur la page entre
  **14:16:12 et 14:16:55**, moins de deux minutes après le chargement. En loops (replay final), le
  premier ordre d'aptitude de chaque `_userid` tombe entre **694 et 1 398**, autour de l'ouverture des
  portes (loop 1 206) : cinq joueurs avant, cinq dans les ~12 s qui suivent.
- `_userid` distincts dans le replay final : `0–9` (les 10 humains) et `16` (non joueur), comme en
  Sandbox pour l'opérateur seul. Les niveaux `allies` / `all` montrent donc bien quelque chose.
- Compteurs finaux (pressions d'aptitude, vue `all`) : Anduin 493, Li-Ming 801, Zagara 561, Genji 255,
  Stitches 381, Arthas 239, Rehgar 248, Azmodan 429, Kel'Thuzad 662, Kerrigan 588.

## Point 2 — cadence des blocs et retards, avec 10 humains

Méthode : la courbe du temps réel est l'enveloppe supérieure des `last_loop` des blocs reçus
(régression linéaire, puis décalage sur le point le plus frais) : **16,23 loops par seconde
d'horloge**. Le retard d'un relevé = (loop « temps réel » à cet instant − loop affiché) / 16,23.
C'est un minorant : il prend pour référence le bloc le plus frais observé, lui-même vieux d'au plus
une seconde ou deux.

| | `tracker.events` | `game.events` (aptitudes) |
|---|---|---|
| Taille finale | 254 936 o | 1 636 416 o |
| Blocs de 4 Ko envoyés | 54 (+ queue) | 396 (+ queue) |
| Intervalle entre blocs | médiane **17 s**, p90 25 s, max 32 s | médiane **2 s**, p90 3 s, max 11 s |
| Âge du dernier événement d'un bloc à l'envoi | médiane 18,7 s, p90 27,3 s, max 41 s | médiane 3,4 s, p90 4,4 s, max 17,4 s |
| **Retard affiché sur la page** (relevés / 2 s) | médiane **28 s**, p90 39 s, max 52 s | médiane **4,7 s**, p90 6,9 s, max 29 s |

**C'est l'inverse de la Sandbox.** Avec 10 humains, `game.events` se remplit en ~2 s : les aptitudes
sont quasi temps réel. Le tracker, lui, reste lent à remplir ses blocs : morts, niveaux et talents
arrivent avec **~30 s** de retard médian, jusqu'à ~50 s. La spec annonçait « ~10 s pour le tracker,
jusqu'à ~85 s pour les aptitudes » d'après la Sandbox. Le critère 2 (< 200 ms côté serveur) n'est pas
touché : ces retards sont ceux du jeu. Le watcher ne retient rien en cours de partie : les 491 relevés
de taille pris pendant la partie sont **tous des multiples de 4 096** pour les deux fichiers (56 et
385 tailles distinctes), donc aucune queue n'attendait l'alignement.

Le dernier bloc partiel est arrivé à la fin : `details` + `initData` vus à **14:32:20** (replay
final écrit à 14:32:20), queue envoyée et `POST /api/live/end` à **14:32:22** (stabilisation de 2 s du
watcher), page en `ended` au relevé de 14:32:23. Aucun `409`, aucun renvoi pendant la partie.

## Point 3 — source du build à mi-partie : **le chemin du process du jeu**

- Pendant toute la partie, le process est
  `C:\Program Files (x86)\Heroes of the Storm\Versions\Base98348\HeroesOfTheStorm_x64.exe` : le
  dossier `BaseNNNNN` du binaire en cours **est** le build de la partie (98348, confirmé par le replay
  final). C'est aussi vrai dès le menu, avant le lobby.
- **Pas dans le battlelobby** : 98348 n'y figure ni en ASCII, ni en u32 petit/grand-boutiste, ni en
  UTF-16 (vérifié sur le battlelobby d'une partie 98348).
- **Le plus haut `Versions\BaseNNNNN` n'est pas une source sûre** : le dossier garde tous les anciens
  builds (Base93640 … Base98348, 24 dossiers) ; le plus haut est le bon aujourd'hui, mais rien ne
  garantit qu'un build plus récent téléchargé ne tourne pas encore, ou l'inverse.
- Non implémenté : le watcher n'envoie pas encore `X-Build` ; le serveur s'est replié sur le dernier
  protocole embarqué (`assumed_latest: true`) et a décodé la partie sans erreur.

## Critère 3 — état live final = projection du replay final : **vérifié**

- **Octets** : `tracker.events`, `game.events` et `battlelobby` du dossier temporaire sont
  **identiques à l'octet** aux streams du replay final, et décodent jusqu'au bout sans reste
  (4 433 événements tracker, 140 168 game events).
- **État** : le replay final uploadé au même serveur (`match_id` 2746) contre la vue live finale,
  par héros : équipe, morts, takedowns et les 7 talents (même `PurchaseName`, même ordre) **concordent
  pour les 10 joueurs** ; niveaux d'équipe 24 / 23 des deux côtés. L'opérateur est déduit du lobby
  (`me_source: lobby`, joueur 9 = Kel'Thuzad), carte « Braxis Outpost ».

## Cycle de vie du dossier (point 5, partiel)

- Le dossier n'existait pas à 14:13 (client lancé à 14:10, dans le menu) ; il est créé au chargement
  de la partie (battlelobby lu à 14:14:59). Celui de la veille (partie de 21:33) avait disparu à
  21:40 (cause non observée). Disparition à la fin d'une partie, client ouvert : non observée.
- Reconnexion en cours de partie : non testée.

## Restent ouverts

- **Point 4** — caler l'horloge (loops − GatesOpen) / 16 sur l'horloge affichée en jeu : demande
  l'heure affichée par l'opérateur à un instant donné, non relevée. Gates ici à loop **1 206** (ARAM),
  contre 610 en Sandbox.
- **Une partie QM ou Storm League** : la cadence du tracker dépend du rythme d'action ; l'ARAM est plus
  dense en combats qu'une partie sur une grande carte. À re-mesurer pour confirmer l'ordre de grandeur.
- **Point 6** (nommage des aptitudes) : pas abordé.

## Constat annexe (hors feed live)

Dans la projection post-partie du replay (`GET /api/matches/2746`), les héros nommés **dans les
événements de mort** (`deaths[].victim.hero`, `killers[].hero`) ne sont pas ceux de la partie : par
exemple le joueur `2-Hero-1-2844614`, Kel'Thuzad dans `players[].hero`, y apparaît en « Kharazim »,
et Murky, Zarya, Medivh, Nova, Tracer, Stukov apparaissent sans avoir été joués. `players[].hero`,
les compteurs et la vue live sont justes. Probablement lié au choix de héros de l'ARAM ; à analyser
côté `storm-stats`.
