# Feed live en partie (`TempWriteReplayP1`) — design

> Spec storm-codex. Rédigée le 2026-10-24 ; **décisions Q1–Q5 tranchées par l'opérateur le
> 2026-10-24** (§ Décisions actées). Origine : question de l'opérateur « peut-on lire le replay en
> direct et voir les aptitudes utilisées ? », puis une session d'observation sur une partie réelle.
> Les mesures sont dans `docs/research/2026-10-24-tempwritereplay-live.md` ; la spec ne les répète
> que là où elles décident quelque chose.
> Plan d'implémentation : `docs/plans/2026-10-24-feed-live.md`.

## But

Pendant une partie, voir **en direct, avec un retard mesuré d'environ 10–14 s pour le tracker et
jusqu'à ~85 s pour les aptitudes** (game events : 37–72 s derrière le tracker), ce qui se passe : niveaux
des équipes, talents choisis, morts avec les crédités, camps et objectifs, et **les aptitudes que
tu utilises**. Aujourd'hui tout cela n'existe qu'après la partie, quand le replay remonte.

## Ce que la mesure a établi (2026-10-24)

Une partie observée : **Sandbox contre l'IA**, Volskaya Foundry, build **98348**.

1. **Le jeu écrit le replay au fil de la partie.** Dans
   `%TEMP%\Heroes of the Storm\TempWriteReplayP1\`, `replay.tracker.events` et
   `replay.game.events` grossissent pendant la partie, par **blocs pleins de 4 Ko**.
2. **Latence = temps de remplir un bloc**, pas un délai fixe :
   - tracker, un bloc toutes les ~10–14 s ;
   - game events, un bloc toutes les ~15–40 s, et rien du tout pendant les ~2 premières minutes
     (le premier bloc n'était pas encore plein). Mesuré 37 à 72 s derrière le tracker.
3. **Ça se décode avec nos tables** : héros, niveaux, talents, morts, camps, objectif, globes, XP
   périodique, positions (toutes les 240 loops), et les ordres d'aptitude (`SCmdEvent.m_abil`).
4. **Le fichier final est le replay.** À la fin de partie, les fichiers temporaires décodent à
   l'identique des streams du `.StormReplay` : 4 519 / 4 519 événements tracker, 6 022 / 6 022 game
   events, `details` et `initData` identiques, battlelobby identique à l'octet. Un parse live mené
   jusqu'au bout donne le parse final.
5. **Fin de partie détectable** : flush final en une fois, avec **apparition de `replay.details` et
   `replay.initData`** (absents pendant la partie). Le dossier **n'est pas supprimé** à la fin ;
   il n'existait pas avant la partie, mais le moment où il disparaît n'a pas été observé.
6. **Le build n'est pas lisible à mi-partie** : pas d'en-tête MPQ dans le dossier. La partie
   tournait en 98348, plus récent que nos tables (96477) ; le repli « dernier protocole connu » a
   suffi cette fois.

**Ce qui n'est PAS établi** (une seule partie, contre l'IA) : la présence en cours de partie des
ordres des 9 autres humains dans une partie matchmakée, les latences avec 10 humains. (L'arrêt
de 13 minutes observé était une pause de l'opérateur : en pause, rien n'est écrit.) → spike
ci-dessous.

## Décision verrouillée touchée ?

Non. La spec programme ne verrouille rien sur le temps réel *en partie* (son « temps réel »
désigne fin de partie → page < 5 s). La décision n° 6 (« pas de mode pré-game ») a déjà été rouverte
par l'opérateur le 2026-08-27 pour le companion ; ce feed en est le prolongement *pendant* la partie.

## Fair-play — tranché par l'opérateur le 2026-10-24

Le tracker **n'est pas filtré par le brouillard de guerre** : `SUnitPositionsEvent` donne la position
des unités des deux équipes toutes les 15 s de jeu ; en partie matchmakée, les game events portent
vraisemblablement les ordres des adversaires (cibles comprises). Afficher en direct une information
que le client ne montre pas au joueur est un avantage, que la source soit un fichier ou la mémoire.
Aucune source n'a été consultée sur la position de Blizzard vis-à-vis de la lecture de ces fichiers
pendant une partie : **le risque CGU est inconnu, pas nul.** L'opérateur a été informé de ce risque
avant de trancher.

**Décision : trois niveaux de visibilité, choisis par l'opérateur sur la page, « moi » par défaut.**

| Niveau (`scope`) | Ce qui est montré en direct |
|---|---|
| `self` — **défaut** | **le tableau des scores** (ce que l'écran Tab montre à tout le monde : héros, niveau d'équipe, takedowns et morts, talents des 10 joueurs, camps capturés, objectifs) + **tes propres aptitudes** |
| `allies` — option | `self` + **les aptitudes de tes 4 alliés** |
| `all` — option, assumée par l'opérateur | `allies` + **les aptitudes des 5 adversaires** |

- Le choix vit dans la page (mémorisé par navigateur) ; un serveur sans paramètre sert `self`.
- Le filtre est appliqué **côté serveur**, à la lecture (`GET /api/live?scope=…`) : une donnée hors
  niveau n'est jamais sérialisée. Le WS ne porte **aucun état** (simple signal d'invalidation),
  donc rien ne fuit par lui.
- **Positions : hors V1 à tous les niveaux**, y compris `all` (cf. Hors scope) — elles ne sont même
  pas conservées par le réducteur.

## Décisions actées (opérateur, 2026-10-24)

| # | Question | Décision |
|---|---|---|
| Q1 | Fair-play | trois niveaux `self` (défaut) / `allies` / `all`, ci-dessus |
| Q2 | Où afficher | **les deux** : une page **`/live`** dans la navigation, et l'état « en partie » de `/companion`. La vue est **un composant unique** (`LiveGame`) monté sur `/live` dès maintenant ; `/companion` (plan 3, pas encore écrit) l'embarquera |
| Q3 | Nommer les aptitudes en V1 | **non** : compteur et timeline par identifiant d'aptitude, nommage = étape 2 |
| Q4 | Où parser | **serveur** ; `client-rs` ne pousse que des octets |
| Q5 | Événements Jarvis en direct | hors V1 |
| — | Ordre de travail | **serveur + page d'abord**, prouvés par un simulateur qui rejoue un replay par morceaux de 4 Ko ; le watcher `client-rs` ensuite ; le spike sur une partie matchmakée n'est **plus bloquant pour coder**, il l'est pour déclarer la feature finie (critère 3) |

## Ce qui existe déjà et qu'on réutilise

| Existant | Usage ici |
|---|---|
| `storm-replay` : `decode_tracker_events` / `decode_game_events` (`src/protocol.rs`) | le décodage lui-même — aujourd'hui privé et non reprenable (cf. contrat ci-dessous) |
| `storm-replay-viewer::extract` : détection des casts (`m_abil` non nul, dédup 0,3 s) | même règle pour compter les aptitudes |
| `storm-stats::process` : lecture de `PlayerSpawned`, `LevelUp`, `TalentChosen`, `PlayerDeath`… | référence de sémantique pour le réducteur live |
| `dim_talents.tree_id` = `PurchaseName` de `TalentChosen` (vérifié : `MonkTranscendence` → Kharazim, tier 1, *Transcendence*) | nom, tier et icône des talents en direct |
| `storm-lobby::parse` + `lobby_live` (`me` = index de l'opérateur) | carte et identification de l'opérateur |
| `/api/upload-raw` (octets bruts + `Bearer`) | modèle de transport |
| canal broadcast `/ws` | nouvel event `live.updated` |
| watcher `client-rs` sur `TempWriteReplayP1` (plan 4 du companion, **pas encore écrit** — absent de `origin/main` de Hots-Overlay au 2026-10-24) | même watcher, élargi aux deux fichiers d'événements |

**Ce qui manque** : le tracker nomme les héros par leur unité (`HeroMonk`, `HeroFaerieDragon`),
pas par le nom canonique (`Kharazim`, `Brightwing`) utilisé partout ailleurs (`dim_heroes.id`). Aucune
table de correspondance n'existe dans le repo. Elle est **dérivée de l'archive** (pour chaque replay :
`PlayerSpawned.Hero` du joueur N ↔ héros du joueur N dans `details`), committée en JSON et embarquée
dans le serveur ; un héros absent de la table s'affiche avec son nom d'unité sans `Hero`.

## Architecture

```
PC de jeu (client-rs, étape suivante)        Box (storm-codex-server)                2e écran
watcher TempWriteReplayP1\
  battlelobby (création)  ── POST /api/lobby ──► (existant) lobby_live ─ lobby.detected
  tracker.events ┐ suit la taille, envoie
  game.events    ┘ les octets ajoutés
        └── POST /api/live/chunk ─────────────► session live (mémoire)
            X-Session, X-Stream, X-Offset         décodeur reprenable par stream
            + octets                               réducteur → LiveState
                                                   └─ WS live.updated (signal) ─► /live, /companion
                                                      GET /api/live?scope=&me= (filtré)
  details + initData apparaissent (fin) ─ POST /api/live/end ─► session close ; le replay remonte
                                                par l'upload existant → match.parsed
```

En attendant le watcher, le **simulateur** (`examples/live_sim.rs` du serveur) tient son rôle : il
lit un `.StormReplay`, poste son battlelobby à `/api/lobby`, puis découpe `tracker.events` et
`game.events` en blocs de 4 Ko qu'il poste dans l'ordre des game loops, au rythme du jeu multiplié
par `--speed`.

### Transport : morceaux ajoutés, indexés par offset

`POST /api/live/chunk`, en-têtes `X-Session: <id>`, `X-Stream: tracker|game`, `X-Offset: <n>`,
`X-Build: <n>` (facultatif), corps = octets bruts ; `Bearer` d'upload.
- **Session** : `X-Session` = **SHA-256 hexadécimal du battlelobby** (`client-rs` comme le
  simulateur). C'est ce qui relie la session au lobby courant (`lobby_live.content_hash`) : la
  carte et le slot de l'opérateur ne sont lus que si les deux coïncident. Un identifiant nouveau
  **remplace** la session courante (une seule partie à la fois).
- **Idempotent** : un morceau déjà reçu (offset + longueur ≤ connu) répond `200 duplicate` ; un
  morceau qui chevauche n'est appliqué que pour sa partie neuve ; un trou (offset > connu) répond
  `409` avec `expected_offset`, le client renvoie depuis là.
- Garde-fous mémoire : 64 Mo au plus par stream et par session (`413` au-delà ; une partie réelle
  en fait quelques centaines de Ko), fil conservé borné à 2 000 éléments, timeline d'aptitudes
  limitée à la fenêtre servie (3 min).

### Contrat de décodage ajouté à `storm-replay`

Le harnais de la mesure a atteint les modules privés par `#[path]` et cherché le préfixe décodable
par force brute : **jetable**. API publique :

```rust
pub enum StreamKind { Tracker, Game }

/// Décodeur d'un stream nu (sans MPQ), alimenté par morceaux.
pub struct StreamDecoder { /* protocole, kind, queue non décodée, gameloop courant */ }

impl StreamDecoder {
    /// `build = None` → dernier protocole embarqué.
    pub fn new(kind: StreamKind, build: Option<u32>) -> Result<StreamDecoder>;
    /// `Some((demandé, utilisé))` si le build n'a pas de table exacte.
    pub fn protocol_fallback(&self) -> Option<(u32, u32)>;
    /// Ajoute des octets, renvoie les événements COMPLETS ; garde la queue coupée pour le
    /// prochain appel. Ne panique jamais.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Value>>;
    pub fn consumed(&self) -> u64;      // octets décodés en événements complets
    pub fn pending(&self) -> usize;     // octets en attente (événement coupé)
    pub fn gameloop(&self) -> i64;      // gameloop du dernier événement complet
}

impl Replay {
    pub fn tracker_events_raw(&self) -> Result<Vec<u8>>;  // stream nu (simulateur, tests)
    pub fn game_events_raw(&self) -> Result<Vec<u8>>;
}
```

- Fin de buffer en plein événement (`Error::Truncated`) → l'événement attend le morceau suivant.
  Toute autre erreur = vraie corruption ou mauvais protocole → `Err`, l'état n'avance plus.
- Changement d'API publique → **`docs/spec/02-storm-replay.md` mis à jour dans le même commit**, et
  test sur le mini-corpus : `feed` par morceaux (4 Ko, 1 octet, tailles pseudo-aléatoires) produit
  exactement `tracker_events()` / `game_events()` du replay entier.

### Le build, à mi-partie

Inconnu tant que `details`/`initData` n'existent pas (fin de partie). Ordre de résolution :
1. `X-Build` s'il est fourni (le spike dira d'où `client-rs` peut le tirer : battlelobby, ou dossier
   d'installation `Versions\BaseNNNNN`) ;
2. à défaut, **dernier protocole embarqué** — ce qui a marché le 2026-10-24 — avec le repli exposé
   dans l'état (`protocol_fallback`).

Si le protocole est faux (patch qui change les streams), le décodage échoue : la session passe en
`unreadable`, jamais d'état inventé.

### Réducteur `LiveState` (pur, zéro I/O)

Module pur du serveur (`src/live/state.rs`), testable sans base ni réseau : il consomme les
événements décodés et maintient l'état. Sémantique alignée sur `storm-stats` (mêmes champs).

- **Base de temps : les game loops** (16 par seconde de jeu). L'horloge affichée est
  `(loop − loop de GatesOpen) / 16` ; elle se cale sur l'horloge en jeu au spike.
- **Joueurs** : `SPlayerSetupEvent` (userId → playerId, slotId) ; héros via `PlayerSpawned` ;
  équipe : joueurs 1–5 = équipe 0, 6–10 = équipe 1 (convention du tracker).
- **Tableau des scores** : niveau par équipe (`LevelUp`), talents par joueur (`TalentChosen`), morts
  (`PlayerDeath` : `PlayerID` + `KillingPlayer`×n → une mort pour la victime, un takedown pour
  chaque crédité), camps (`JungleCampCapture`), objectifs (autres `SStatGameEvent` de carte, gardés tels
  quels dans le fil).
- **Aptitudes** : par joueur, par couple `(m_abilLink, m_abilCmdIndex)` : compteur + gameloops
  (dédup 0,3 s par couple, règle de la visionneuse).
- **Opérateur (`me`)** — un « me » faux montrerait en `self` les aptitudes d'un autre joueur, un
  « me » indéterminé n'en montre aucune ; d'où, dans l'ordre : `?me=` explicite (sélecteur de la
  page) ; slot de l'opérateur dans le lobby **de cette session** (hash = id) → playerId via
  `SPlayerSetupEvent.m_slotId` (ordre du lobby = slot, vérifié sur une partie à 10) ; si ce lobby
  compte plusieurs humains, rien ; sinon l'unique joueur humain ayant émis un game event, une fois
  que les game events couvrent au moins une minute (partie contre l'IA). Test : à aucun bloc d'une
  partie à 10 humains cette dernière règle ne se déclenche.
- `last_loop` par stream et horodatage du dernier morceau reçu, exposés tels quels : la page
  affiche le retard plutôt que de laisser croire à du temps réel.

### Aptitudes

Un ordre d'aptitude est un `SCmdEvent` avec `m_abil` = `(m_abilLink, m_abilCmdIndex)`. Deux limites
affichées sur la page :
- c'est une **pression de touche**, pas un lancer réussi (portée, recharge, annulation non connues) ;
- `m_abilLink` est un index dans le catalogue d'aptitudes **du build** : son nom n'est pas dans le
  replay. Le 2026-10-24, Kharazim a produit 13 couples distincts en 10 minutes.

V1 (**Q3**) : compteur et timeline par couple, sans nom. Le nommage est une étape 2 avec deux pistes
à évaluer : extraction depuis les données du jeu (CASC, lourd, à refaire à chaque patch) ou table
calibrée par héros (presser chaque touche en mode essai, une fois par patch).

## API et événements

| Route / event | Rôle |
|---|---|
| `POST /api/live/chunk` 🔒 | octets ajoutés d'un stream (`X-Session`, `X-Stream`, `X-Offset`, `X-Build`?), `Bearer` upload. `200` `{status: applied\|duplicate, consumed, last_loop}` ; `409` `{expected_offset}` ; `400` en-tête invalide |
| `POST /api/live/end` 🔒 | `{session}` — fin de partie signalée par le client ; la session passe en `ended` |
| `GET /api/live?scope=self\|allies\|all&me=<playerId>` | vue filtrée de la session courante ; `204` si aucune |
| `DELETE /api/live` 🔒 | ferme la session (garde `ADMIN_TOKEN` des routes de gestion) |
| WS `live.updated` | `{type, session}` après chaque morceau qui change l'état ou changement de statut ; **aucun état dans l'event** — le front invalide `["live"]` |

Pas de table : l'état live est **en mémoire** et jetable. Le replay archivé reste la source de
vérité (étage 1) ; la projection Postgres vient du parse final, inchangé. Cohérent avec la règle
« jamais de pré-décodage massif ».

## Front

Composant **`LiveGame`** (vue complète d'une partie en cours), monté sur la page **`/live`**
(entrée « Live » de la navigation) et destiné à l'état « en partie » de `/companion` quand cette page
existera. Piloté par `live.updated` :
- **bandeau** : carte, horloge de jeu, retard des données (tracker / aptitudes / dernier morceau
  reçu), statut (`live`, `ended`, `unreadable`, flux interrompu) ;
- **visibilité** : sélecteur `Moi + tableau des scores` / `+ alliés` / `Tout` (avertissement affiché
  sur `Tout`), et sélecteur « je suis » (10 héros) quand l'opérateur n'est pas déduit ;
- **tableau des scores** : deux équipes, héros (portrait), niveau d'équipe, takedowns / morts, talents par tier ;
- **aptitudes** : par joueur visible, une ligne par identifiant (compteur + timeline des 3 dernières
  minutes), avec la mention « pressions de touche, non nommées » ;
- **fil** : morts, talents, camps, objectifs, du plus récent au plus ancien ;
- **au repos** : « en attente d'une partie » — jamais une page vide.

## Dégradations (comportements exigés)

| Ce qui casse | Comportement |
|---|---|
| Fichiers d'une partie déjà finie trouvés au démarrage de `client-rs` (`replay.details` présent) | partie terminée → **aucune session live ouverte** ; le replay remonte par l'upload existant (même piège que le battlelobby résiduel du companion) |
| Protocole faux (patch) | session `unreadable` (« flux illisible, build supposé X ») ; plus aucun état ne change ; le reste de l'app marche |
| Game events encore vides (début de partie) | section aptitudes « en attente des premières données », pas un zéro |
| Aucun morceau depuis > 60 s alors que la session n'est pas `ended` | « flux interrompu (pause ou déconnexion ?) » — une pause de 13 min observée le 2026-10-24 produit exactement ce silence ; la page ne peut pas distinguer pause et déconnexion |
| Héros absent de la table d'unités | nom d'unité sans `Hero` (ex. `Monk`), jamais une case vide |
| Box injoignable | le client garde son offset et rattrape au retour (`409` → renvoi) ; la page affiche l'état précédent marqué périmé |
| `client-rs` absent ou arrêté | rien ne change pour le post-partie : le replay remonte comme aujourd'hui |

## Spike (sur le PC de jeu — requis avant de déclarer la feature finie, plus avant de coder)

1. **Une partie matchmakée** (QM ou Storm League) : les ordres des 10 joueurs sont-ils dans le
   `game.events` temporaire **en cours de partie** ? (Décide si `allies`/`all` montrent réellement
   quelque chose.)
2. Re-mesurer les deux latences et la cadence des blocs avec 10 humains.
3. **Source du build à mi-partie** : chercher le build dans le battlelobby ; à défaut, vérifier la
   lecture de `Versions\BaseNNNNN` dans le dossier d'installation.
4. Caler l'horloge (loops − GatesOpen) sur l'horloge affichée en jeu.
5. Cycle de vie du dossier : quand disparaît-il (fermeture du client, partie suivante) ? En cas de
   reconnexion en cours de partie, est-il recréé, les fichiers repartent-ils de zéro ?
6. Aptitudes, étape 2 : évaluer la faisabilité des deux pistes de nommage (sans les implémenter).

## Tests

- **`storm-replay`** : `StreamDecoder::feed` par morceaux (4 Ko, 1 octet, tailles pseudo-aléatoires)
  = décodage du replay entier, sur le mini-corpus ; jamais de panique sur entrée tronquée ou
  aléatoire.
- **Réducteur** : rejouer un replay committé morceau par morceau ; l'état final concorde avec le
  parse `storm-stats` du même replay (héros, niveaux d'équipe, talents, morts).
- **Filtre de visibilité** : tests qui échouent si une aptitude hors niveau (allié en `self`,
  adversaire en `allies`) apparaît dans la vue sérialisée.
- **Serveur** : idempotence de `/api/live/chunk` (doublon ignoré, chevauchement, trou → `409`),
  remplacement de session.
- **E2E rejouable sans partie** : `live_sim` sur un replay → la page `/live` passe par les états
  attendus (vérifié dans le navigateur).

## Critères d'acceptation

1. Spike : points 1 à 4 tranchés, chiffres consignés dans `docs/research/`.
2. Morceau reçu → `live.updated` émis **< 200 ms** (le retard dominant est celui du jeu, ~10 s
   pour le tracker, jusqu'à ~85 s pour les aptitudes, hors de notre contrôle). **Mesuré le
   2026-10-24** (aller-retour `POST /api/live/chunk`, qui répond après le broadcast ; serveur release
   local) : p95 1,7 ms sur 76 blocs, 1,6 ms sur 259 blocs → **tenu**.
3. En fin de partie, l'état live final concorde avec la projection du replay final (héros, niveaux,
   talents, morts), vérifié sur une partie réelle.
4. Aucune aptitude hors niveau dans la vue servie (test automatisé).

## Hors scope V1

- Overlay OBS du feed live.
- **Positions et minimap en direct, à tous les niveaux de visibilité** (étape suivante possible,
  derrière le niveau `all`).
- Événements Jarvis en direct.
- Nommage des aptitudes (étape 2).
- Lecture de la mémoire du jeu ou toute autre source que les fichiers du dossier temporaire.
