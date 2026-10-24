# Feed live en partie (`TempWriteReplayP1`) — design

> Spec storm-codex. **BROUILLON du 2026-10-24 — à valider par l'opérateur.** Rien n'est implémenté.
> Origine : question de l'opérateur « peut-on lire le replay en direct et voir les aptitudes
> utilisées ? », puis une session d'observation sur une partie réelle. Les mesures sont dans
> `docs/research/2026-10-24-tempwritereplay-live.md` ; la spec ne les répète que là où elles
> décident quelque chose.
> Les points marqués **[opérateur]** sont des décisions à trancher, avec une proposition par défaut.

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

## Fair-play — à trancher en premier **[opérateur]**

Le tracker **n'est pas filtré par le brouillard de guerre** : `SUnitPositionsEvent` donne la position
des unités des deux équipes toutes les 15 s de jeu ; en partie matchmakée, les game events portent
vraisemblablement les ordres des adversaires (cibles comprises). Afficher en direct une information
que le client ne montre pas au joueur est un avantage, que la source soit un fichier ou la mémoire.
Aucune source n'a été consultée sur la position de Blizzard vis-à-vis de la lecture de ces fichiers
pendant une partie : **le risque CGU est inconnu, pas nul.**

**Proposition par défaut** : en direct, n'afficher que
- **tes propres données** (tes aptitudes, tes talents, tes morts) ;
- **ce que le tableau des scores en jeu montre déjà** à tout le monde : niveaux d'équipe, talents
  des 10 joueurs, morts et crédités, camps capturés, objectifs.

Et ne **jamais** afficher en direct : positions (aucune équipe), ordres ou aptitudes des
adversaires, unités hors vision. Ces données restent **post-partie** (visionneuse 2D, fiche de match),
comme aujourd'hui. Le filtre vit **côté serveur** : ce qui n'est pas autorisé n'est jamais émis sur
le WS, pas simplement masqué par le front.

## Questions ouvertes pour l'opérateur

| # | Question | Proposition par défaut |
|---|---|---|
| Q1 | Fair-play : périmètre affiché en direct | ci-dessus : toi + tableau des scores |
| Q2 | Audience | `/companion` (2e écran, privé) — un 3e état « en partie » entre pré-game et debrief. Overlay OBS plus tard, et seulement avec le même filtre |
| Q3 | Nommer les aptitudes (Q/W/E/R/D/monture/foyer) en V1 ? | **non** en V1 (cf. § Aptitudes) : compteur et timeline par touche non nommée, nommage = étape 2 |
| Q4 | Où parser | **serveur** (comme le companion, décision 6 du 2026-08-27) : `client-rs` ne fait que pousser des octets |
| Q5 | Événements Jarvis en direct (« 3e mort », « niveau 10 atteint ») | hors V1 ; le flux le rendra trivial ensuite |

## Ce qui existe déjà et qu'on réutilise

| Existant | Usage ici |
|---|---|
| `storm-replay` : `decode_tracker_events` / `decode_game_events` / `visit_game_events` (`src/protocol.rs`) | le décodage lui-même — mais aujourd'hui privé et non reprenable (cf. contrat ci-dessous) |
| `storm-replay-viewer::extract` : détection des casts (`m_abil` non nul, dédup 0,3 s) | même règle pour compter les aptitudes |
| `storm-stats::process` : lecture de `PlayerSpawned`, `LevelUp`, `TalentChosen`, `PlayerDeath`… | référence de sémantique pour le réducteur live (pas de second interprète divergent) |
| `storm-lobby::parse` + `lobby_live` + `POST /api/lobby` | identité de la partie en cours (le battlelobby est complet dès la création du dossier) |
| `/api/upload-raw` (octets bruts + `Bearer`) | modèle de transport |
| canal broadcast `/ws` | nouvel event `live.updated` |
| `/companion` (plan 3) | la page qui accueille l'état « en partie » |
| **à construire de toute façon** : watcher `client-rs` sur `TempWriteReplayP1` (plan 4 du companion, **pas encore écrit** — absent de `origin/main` de Hots-Overlay au 2026-10-24) | même watcher, élargi aux deux fichiers d'événements |

## Architecture

```
PC de jeu (client-rs)                         Box (storm-codex-server)                2e écran
watcher TempWriteReplayP1\
  battlelobby (création)  ── POST /api/lobby ──► (existant) lobby_live ─ lobby.detected ─► /companion
  tracker.events ┐ suit la taille, envoie
  game.events    ┘ les octets ajoutés
        └── POST /api/live/chunk ─────────────► session live (mémoire)
            {stream, offset} + octets              décodeur reprenable par stream
                                                   réducteur → LiveState
                                                   filtre fair-play
                                                   └─ WS live.updated ──────────────────► état « en partie »
  details + initData apparaissent (fin)  ─────► session close ; le replay remonte par
                                                l'upload existant → match.parsed ─────► debrief
```

### Transport : morceaux ajoutés, indexés par offset

`client-rs` mémorise par stream l'offset déjà envoyé et ne poste que **les octets ajoutés** :
`POST /api/live/chunk`, en-têtes `X-Stream: tracker|game`, `X-Offset: <n>`, corps = octets bruts.
- **Idempotent** : un morceau déjà reçu (offset + longueur ≤ connu) est accepté et ignoré ; un trou
  (offset > connu) répond `409` avec l'offset attendu, le client renvoie depuis là.
- Pas de re-POST du fichier entier (il atteint ~240 Ko en 10 min de jeu et croît).
- La session est identifiée par le hash du battlelobby (le même hash que le watcher du companion,
  plan 4, doit calculer pour sa dédup) ; un nouveau lobby ferme la session précédente.

### Contrat de décodage à ajouter dans `storm-replay`

Le harnais de la mesure a atteint les modules privés par `#[path]` et a cherché le préfixe décodable
par force brute : **jetable**. Il faut une API publique :

```rust
/// Décodeur d'un stream nu (sans MPQ), alimenté par morceaux.
pub struct StreamDecoder { /* protocole, kind, buffer, checkpoint (offset, gameloop) */ }

impl StreamDecoder {
    pub fn new(kind: StreamKind, build: u32) -> Result<(StreamDecoder, Fallback)>;
    /// Ajoute des octets, renvoie les événements COMPLETS ; garde la queue coupée pour le
    /// prochain appel. Ne panique jamais sur entrée tronquée.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Value>>;
}
pub enum StreamKind { Tracker, Game }
```

- Reprend au dernier événement complet (`(offset, gameloop)` du checkpoint), au lieu de redécoder
  tout le buffer à chaque morceau. Une erreur **au milieu** du buffer (pas en fin) = vraie corruption
  ou mauvais protocole → `Err` classé.
- Changement d'API publique de `storm-replay` → **`docs/spec/02-storm-replay.md` mis à jour dans le
  même commit** (règle 0), et crosscheck sur le mini-corpus : `feed` par morceaux de 4 Ko doit
  produire exactement `tracker_events()` / `game_events()` du replay entier.

### Le build, à mi-partie

Inconnu tant que `details`/`initData` n'existent pas (fin de partie). Ordre de résolution :
1. ce que le **spike** trouve (le battlelobby, ou le dossier d'installation `Versions\BaseNNNNN` lu
   par `client-rs` et envoyé avec le premier morceau) ;
2. à défaut, **dernier protocole embarqué** — ce qui a marché le 2026-10-24 — avec le repli signalé.

Si le protocole est faux (patch qui change les streams), le décodage échoue au milieu du buffer :
dégradation classée (ci-dessous), jamais d'état inventé.

### Réducteur `LiveState` (pur, zéro I/O)

Module pur, testable sans serveur, qui consomme les événements décodés et maintient l'état. Il
réutilise la sémantique de `storm-stats` (mêmes champs, mêmes clés) — emplacement exact (module de
`storm-stats` ou crate dédié) tranché au plan d'implémentation.

- **Base de temps : les game loops** (16 par seconde de jeu). L'horloge affichée est
  `(loop − loop de GatesOpen) / 16` ; elle se cale sur l'horloge en jeu au spike.
- Contenu : 10 joueurs (héros), niveau par équipe, talents par joueur, morts/crédités, camps,
  objectifs de carte, et pour l'opérateur seul : ses ordres d'aptitude (compteur et timeline).
- `last_loop` par stream, exposé tel quel : la page affiche « données à −Ns » plutôt que de
  laisser croire à du temps réel.

### Aptitudes

Un ordre d'aptitude est un `SCmdEvent` avec `m_abil` = `(m_abilLink, m_abilCmdIndex)`. Deux limites
à afficher honnêtement :
- c'est une **pression de touche**, pas un lancer réussi (portée, recharge, annulation non connues) ;
- `m_abilLink` est un index dans le catalogue d'aptitudes **du build** : son nom n'est pas dans le
  replay. Le 2026-10-24, Kharazim a produit 13 couples distincts en 10 minutes.

V1 proposée (**Q3**) : compteur et timeline par couple, regroupés par héros, sans nom. Le nommage
est une étape 2 avec deux pistes à évaluer au spike : extraction depuis les données du jeu (CASC,
lourd, à refaire à chaque patch) ou table calibrée par héros (presser chaque touche en mode essai,
une fois par patch).

## API et événements

| Route / event | Rôle |
|---|---|
| `POST /api/live/chunk` 🔒 | octets ajoutés d'un stream (`X-Stream`, `X-Offset`), `Bearer` upload. `200` (accepté/ignoré), `409` + offset attendu |
| `POST /api/live/end` 🔒 | fin de partie signalée par `client-rs` (apparition de `details`) ; ferme la session |
| `GET /api/live` | `LiveState` filtré courant ; `204` si aucune partie |
| WS `live.updated` | après chaque morceau décodé qui change l'état ; consommé comme `lobby.updated` (invalidation TanStack Query) |

Pas de table : l'état live est **en mémoire** et jetable. Le replay archivé reste la source de
vérité (étage 1) ; la projection Postgres vient du parse final, inchangé. Cohérent avec la règle
« jamais de pré-décodage massif ».

## Front

État **« en partie »** de `/companion`, entre pré-game et debrief, piloté par `live.updated` :
bandeau horloge de jeu et retard des données ; niveaux d'équipe ; tes aptitudes (compteur, timeline
des 2 dernières minutes) ; fil des événements (morts, talents, camps, objectifs). Rien d'autre
tant que Q1 n'est pas tranché. À la fin, bascule vers le debrief existant quand `match.parsed`
arrive.

## Dégradations (comportements exigés)

| Ce qui casse | Comportement |
|---|---|
| Fichiers d'une partie déjà finie trouvés au démarrage de `client-rs` (`replay.details` présent) | partie terminée → **aucune session live ouverte** ; le replay remonte par l'upload existant (même piège que le battlelobby résiduel du companion) |
| Protocole faux (patch) | session marquée « flux illisible (build supposé X) », plus aucun état émis ; le pré-game et le debrief continuent de marcher |
| Game events encore vides (début de partie) | section aptitudes « en attente des premières données », pas un zéro |
| Aucun morceau depuis > 60 s alors que la partie n'est pas finie | « flux interrompu (pause ou déconnexion ?) » — une pause de 13 min observée le 2026-10-24 produit exactement ce silence ; la page ne peut pas distinguer pause et déconnexion |
| Box injoignable | `client-rs` garde son offset et rattrape au retour ; la page affiche l'état précédent marqué périmé |
| `client-rs` absent ou arrêté | rien ne change pour le post-partie : le replay remonte comme aujourd'hui |

## Spike (préalable, sur le PC de jeu, aucune ligne de serveur ni de front avant)

1. **Une partie matchmakée** (QM ou Storm League) : les ordres des 10 joueurs sont-ils dans le
   `game.events` temporaire **en cours de partie** ? Décide la portée réelle du filtre Q1.
2. Re-mesurer les deux latences et la cadence des blocs avec 10 humains.
3. **Source du build à mi-partie** : chercher le build dans le battlelobby ; à défaut, vérifier la
   lecture de `Versions\BaseNNNNN` dans le dossier d'installation.
4. Caler l'horloge (loops − GatesOpen) sur l'horloge affichée en jeu.
5. Cycle de vie du dossier : quand disparaît-il (fermeture du client, partie suivante) ? En cas de
   reconnexion en cours de partie, est-il recréé, les fichiers repartent-ils de zéro ?
6. Aptitudes, étape 2 : évaluer la faisabilité des deux pistes de nommage (sans les implémenter).

## Tests

- **`storm-replay`** : `StreamDecoder::feed` par morceaux (4 Ko, 1 octet, tailles aléatoires) =
  décodage du replay entier, sur le mini-corpus ; jamais de panique sur entrée tronquée ou
  aléatoire.
- **Réducteur** : rejouer les événements d'un replay committé morceau par morceau ; l'état final
  concorde avec la projection `storm-stats` du même replay (niveaux, talents, morts).
- **Filtre fair-play** : test qui échoue si un champ interdit (position, ordre adverse) apparaît
  dans le `LiveState` sérialisé.
- **Serveur** : idempotence de `/api/live/chunk` (doublon ignoré, trou → `409`).
- **E2E rejouable sans partie** : découper un replay committé en morceaux de 4 Ko et les poster au
  rythme mesuré → la page passe par les états attendus.

## Critères d'acceptation

1. Spike : points 1 à 4 tranchés, chiffres consignés dans `docs/research/`.
2. Morceau reçu → `live.updated` émis **< 200 ms** (le retard dominant est celui du jeu, 10–70 s,
   hors de notre contrôle).
3. En fin de partie, l'état live final concorde avec la projection du replay final (niveaux,
   talents, morts), vérifié sur une partie réelle.
4. Aucun champ hors périmètre Q1 sur le WS (test automatisé).

## Hors scope V1

- Overlay OBS du feed live.
- Positions et minimap en direct.
- Événements Jarvis en direct.
- Nommage des aptitudes (étape 2).
- Lecture de la mémoire du jeu ou toute autre source que les fichiers du dossier temporaire.
