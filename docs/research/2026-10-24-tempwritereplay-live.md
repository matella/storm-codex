# Fichiers `TempWriteReplayP1` pendant une partie — mesures du 2026-10-24

> Constats bruts d'une session d'observation sur le PC de jeu, pour la spec
> `docs/specs/2026-10-24-feed-live-en-partie-design.md`. **Une seule partie observée** : une partie
> **« Sandbox » contre l'IA** (titre du replay final `Sandbox (Volskaya Foundry)` ; les 9 autres
> joueurs sont `Player N`, toon `0-Hero-0-0`), build **98348** (2.57.0). Rien ici n'est généralisé
> à une partie matchmakée avant re-mesure.

## Protocole d'observation

- **Watcher** PowerShell : liste `%TEMP%\Heroes of the Storm\TempWriteReplayP1\` toutes les 2 s et
  journalise chaque fichier nouveau, modifié ou disparu. La taille est lue **en ouvrant le fichier**
  (lecture partagée `ReadWrite, Delete`), pas dans le listing du répertoire, qui peut être périmé
  pour un fichier gardé ouvert par le jeu.
- **Copies** : instantanés des fichiers (même ouverture partagée) à 16:21, 16:23:02, 16:23:39 et
  16:26:48 ; miroir rafraîchi toutes les 3 s ensuite.
- **Décodage** : harnais jetable qui réutilise les modules privés de `storm-replay`
  (`protocol`, `versioned`, `bitpacked`…) par `#[path]`, plus `storm-lobby::parse` pour le
  battlelobby. Le préfixe décodable le plus long est trouvé par force brute (le dernier événement
  est souvent coupé). Le harnais n'est **pas** dans le repo — c'est une preuve de faisabilité.
- **Protocole** : tables du build `96477` (dernier build embarqué). ⚠ Le dossier temporaire n'a
  **pas d'en-tête MPQ**, donc pas de `m_baseBuild` : le build a été **supposé**, pas lu. Le replay
  final a révélé après coup que la partie tournait en **98348** — plus récent que nos tables. Le
  décodage a fonctionné quand même (même repli « dernier protocole connu » que le parse serveur),
  ce qui veut dire que le protocole n'a pas changé entre les deux builds pour ces streams, pas que
  ce sera toujours le cas.

## Ce que contient le dossier

Créé à **16:04:35** (tous les fichiers en même temps) ; observé depuis 16:05:28.

| Fichier | Pendant la partie |
|---|---|
| `replay.server.battlelobby` | 279 740 octets, complet dès la création, ne bouge plus |
| `replay.tracker.events` | **grossit pendant la partie**, par blocs de 4 096 octets |
| `replay.game.events` | **grossit pendant la partie**, par blocs de 4 096 octets, démarrage tardif (cf. plus bas) |
| `replay.attributes.events`, `replay.load.info`, `replay.message.events`, `replay.resumable.events`, `replay.smartcam.events`, `replay.sync.events`, `replay.sync.history` | **0 octet** pendant toute l'observation |

Le battlelobby, décodé par `storm-lobby` : carte `Volskaya Foundry`, **un seul** joueur
(`matella#21409`, équipe `None`).

## Cadence d'écriture

Les écritures se font par **blocs pleins de 4 Ko** : la latence est le temps de remplir un bloc,
pas un délai fixe.

**`replay.tracker.events`**
- 20 480 octets dès 16:05:47 (rafale de `SUnitBornEvent` du début de partie : structures, sbires).
- Ensuite un bloc toutes les **~10–14 s** (16:05:53 → 16:07:46, puis 16:21:05 → fin d'observation).
- Débit mesuré entre deux instantanés : 649 game loops pour 37 s d'horloge (16:23:02 → 16:23:39),
  soit l'avancement du jeu en temps réel.

**`replay.game.events`**
- **0 octet jusqu'à 16:21:41**, alors que les loops 0 → ~2 000 avaient déjà été joués avant la pause
  (cf. plus bas). Le premier bloc n'est écrit qu'une fois 4 Ko accumulés.
- Ensuite un bloc toutes les **~15–40 s** (dépend de l'activité : caméra, ordres).
- Retard sur le tracker, au même instant : **~37 s** (instantané 16:23:39 : tracker loop 5 584,
  game loop 4 998) et **~72 s** (16:26:48 : tracker 8 643, game 7 482).

**Arrêt de 13 minutes** : aucune écriture ni avancée des loops entre **16:07:46 et 16:21:05**, sur
les deux fichiers, sans disparition du dossier. **Cause : l'opérateur avait mis la partie en
pause** (confirmé par lui le 2026-10-24). En pause, le jeu n'écrit rien et les loops n'avancent
pas ; l'écriture reprend avec la partie, sans trou ni rattrapage.

**Bord de bloc** : le dernier événement d'un fichier est généralement coupé ; 1 à 14 octets restaient
non décodables en fin de buffer selon l'instantané.

## Ce qui se décode à mi-partie

Instantané de 16:26:48 (loop 8 643 côté tracker, 7 482 côté game ; gates à loop 610).

**Tracker** — 4 103 événements : `SUnitBornEvent`, `SUnitDiedEvent`, `SUnitOwnerChangeEvent`,
`SUnitPositionsEvent`, `SUnitRevivedEvent`, `SUnitTypeChangeEvent`, `SUpgradeEvent`,
`SPlayerSetupEvent` (×10), `SStatGameEvent`.
- **Héros** des 10 joueurs via `PlayerSpawned` (ex. P1 `HeroMonk`, P8 `HeroChromie`).
- **Niveaux** (`LevelUp`, par joueur), **talents** (`TalentChosen` avec `PurchaseName`, ex.
  `ChromieSandBlastOnceAgainTheFirstTime`), **morts** avec tous les crédités (`PlayerDeath` :
  `PlayerID` + `KillingPlayer`×n), **camps** (`JungleCampCapture`, `CampType`), **objectif de
  carte** (`VolskayaCapturePointSpawned/Complete`), **globes** (`RegenGlobePickedUp`), **XP
  périodique** (`PeriodicXPBreakdown`).
- **Positions** : `SUnitPositionsEvent` toutes les **240 loops** (loops 960, 1 200, 1 440…), pour
  les unités suivies **des deux équipes** — le tracker n'est pas filtré par le brouillard de guerre.

**Game events** — 4 927 événements : `SCameraUpdateEvent`, `SCmdEvent`,
`SCmdUpdateTargetPointEvent`, `SCommandManagerStateEvent`, `STriggerKeyPressedEvent`,
`SHeroTalentTreeSelectedEvent`…
- **Un seul `_userid` (0)** émet des ordres (≈ 2 événements sous `_userid` 16, non joueur).
  `SPlayerSetupEvent` : userId 0 → player 1 (Kharazim).
- **Aptitudes** : `SCmdEvent` avec `m_abil` non nul — à 7:47 de loops, P1 compte 123 ordres
  d'aptitude répartis sur 13 couples `(m_abilLink, m_abilCmdIndex)` (ex. `26/0` ×43, `491/0` ×16).
  Ce sont des **pressions de touche**, pas des lancers confirmés. **Quel `m_abilLink` correspond à
  quelle touche n'est pas connu** : l'identifiant pointe dans le catalogue d'aptitudes du build.

## Fin de partie

- Replay final écrit à **16:27:41** :
  `Replays/Multiplayer/2026-10-24 16.27.41 Sandbox (Volskaya Foundry).StormReplay` (141 625
  octets, 9 736 loops, victoire).
- À **16:27:43**, flush final en une fois dans le dossier temporaire : dernier bloc partiel de
  `tracker.events` (241 687 octets, plus un multiple de 4 Ko) et de `game.events` (64 264), et
  **apparition** de `replay.details` (1 408) et `replay.initData` (17 073) ; `attributes.events`,
  `load.info`, `resumable.events`, `smartcam.events`, `sync.events` passent de 0 à quelques
  centaines d'octets. `message.events` et `sync.history` restent vides (aucun chat).
- **Le dossier n'est pas supprimé** à la fin de la partie (toujours présent à 16:29). Il
  n'existait pas avant la partie (vérifié vers 16:00) et est apparu à 16:04:35, tous fichiers
  créés en même temps. Quand il disparaît (fermeture du client ? lancement de la partie
  suivante ?) **n'a pas été observé**.
- **Identité avec le replay final**, décodée avec les tables du build final : tracker **4 519 / 4 519
  événements identiques**, game events **6 022 / 6 022 identiques**, `details` et `initData`
  identiques, `battlelobby` **identique à l'octet**. Les streams d'événements ont été comparés
  **décodés**, pas octet par octet : ils décodent à l'identique, donc un parse live mené jusqu'au
  bout donne le parse final.
- Le replay final ne contient lui aussi que les `_userid` 0 et 16 : en Sandbox, les IA n'émettent
  pas de game events. Ce n'est pas une particularité du fichier temporaire.

## Limites

1. **Une partie Sandbox contre l'IA** : un seul humain, donc un seul émetteur d'ordres (l'arrêt de
   13 min est une pause volontaire, cf. Cadence d'écriture). Dans une partie matchmakée, le volume d'ordres (10 humains) change le
   temps de remplissage des blocs, donc les latences ; la présence des ordres **des 9 autres**
   dans le fichier temporaire en cours de partie reste à vérifier.
2. **Build non lisible à mi-partie** (cf. Protocole d'observation) : appris seulement à la fin, par
   le replay final ou `replay.initData`/`details` qui n'apparaissent qu'au flush final.
