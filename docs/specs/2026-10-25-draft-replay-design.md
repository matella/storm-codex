# Draft depuis un replay — design

> Spec storm-codex, à valider par l'opérateur avant tout code (Règle n° 0). Brainstorming du
> 2026-10-25. S'appuie sur le simulateur de draft existant (`docs/specs/2026-06-19-draft-simulator-design.md`).
> Les points marqués **[choix Claude]** sont des décisions prises sans l'opérateur : à relire.

## But

L'opérateur caste des parties déjà jouées : il n'y a pas de draft en direct. Il charge un fichier
`.StormReplay`, le draft de cette partie est **préparé** (carte, joueurs, séquence complète des
bans/picks) mais **caché**, puis il le **déroule à la main** (ou en lecture auto). L'écran vu par
le public est l'overlay OBS existant `/draft/overlay` ; la nouvelle page n'est que la télécommande.

## Décisions actées (avec l'opérateur)

1. **Page dédiée** `/draft/replay` : on y choisit/dépose un `.StormReplay` ; le draft est préparé à
   l'étape 0 (rien de révélé), puis démarré manuellement.
2. **Sortie = overlay OBS existant**, réutilisé tel quel (même état de draft, même WS `draft.updated`).
3. **Dans le périmètre V1** : lecture auto à minuterie, file de plusieurs replays (une série),
   et fearless sur cette série (les picks des parties précédentes deviennent indisponibles).

## Ce que le replay fournit (déjà extrait par storm-stats, `process_draft`)

`match.bans` et `match.picks` par équipe, dans l'ordre de chaque équipe ; `picks.first` (équipe
first-pick) ; pour chaque joueur son héros et son pseudo. L'ordre global des 16 étapes est
reconstruit par la séquence fixe du format Standard (même table que `selections` du parser).
Les bans sont des codes attribut (`Crus`…) à résoudre via `/api/dim/hero-attributes` ; les picks
sont déjà des noms canoniques. Clé héros du simulateur = nom canonique (inchangé).

Builds < 66292 : séquence de bans différente (2 bans par équipe en 1re phase, marqués « N/A ») —
les étapes sans héros sont **sautées** (voir « étape vide »).

## Conception

### Moteur (`draft/mod.rs`) — extension minimale

- `DraftState` gagne `replay: Option<ReplayScript>` (`#[serde(default)]`, `schema_version` 2) :
  `{ source: String, script: Vec<Option<String>>, players: Vec<Option<String>> , playing: bool, interval_ms: u32 }`
  — `script[i]` = héros de l'étape i (`None` = étape vide), `players[i]` = pseudo du joueur du pick.
- `reveal_next()` : applique `script[cursor]` (ou saute si `None`) ; réutilise `apply`/curseur. `undo`
  et `reset` existants servent de retour arrière. `assignments` ne contient **que le déjà révélé** :
  l'overlay n'affiche donc jamais le futur. Le script complet n'est servi qu'à la page replay.
- **Étape vide** : ajout d'un `skip()` (curseur +1, assignation `None`), affichée « no ban » comme sur
  la fiche de match.
- Le format du draft préparé est **Standard** ; **Fearless** si la file est en mode fearless.
  `first_pick` / côtés viennent du replay (équipe 0 = Blue, équipe 1 = Red **[choix Claude, à vérifier
  sur un replay réel]**).

### Décodage

- `POST /api/draft/replay/load` (octets bruts, comme `/api/upload-raw`) : décode **uniquement** le draft
  (storm-replay + `process_draft`), **sans écrire la partie en base** **[choix Claude]** : charger un
  replay pour caster ne doit pas polluer les stats. Réponse : métadonnées (map, joueurs, build, script).
- Refus explicite (4xx + message) pour les replays sans draft (Quick Match, ARAM, IA).
- Budget : < 500 ms pour un replay (parse < 150 ms + marge réseau local).

### File de replays (série)

- `replay_queue: Vec<LoadedGame>` (picks/bans/joueurs de chaque partie) dans l'état, persistée comme
  le reste (`draft_live`). Routes : ajout (`load`), retrait, réordonnancement, « jouer la partie N ».
- Une partie terminée → « partie suivante » = `start_next_game()` existant : en mode fearless, les
  picks cumulés des parties précédentes passent en `series_bans` (ruban déjà géré par l'overlay).
- Le score et les noms d'équipe restent manuels (le replay n'en contient pas) **[choix Claude]** ; les
  pseudos des 5 joueurs de chaque camp sont préremplis depuis le replay (mappés au pick de leur héros).

### Lecture

- **Manuel** : bouton Suivant / touche Espace, Retour (undo), Reset. Un clic = une étape révélée.
- **Auto** : Play/Pause + délai par étape réglable (défaut 3 s **[choix Claude]**), timer côté serveur
  (autoritatif, comme l'état) pour résister au rafraîchissement ; s'arrête à la fin du draft.
- Raccourcis clavier actifs même quand la page n'a pas le focus OBS (la page replay est dans un
  navigateur à part de la browser source) : rien à faire côté OBS.

### Routes et événements

`POST /api/draft/replay/load`, `/replay/next`, `/replay/back`, `/replay/play` `{playing, interval_ms}`,
`/replay/queue/*` ; `GET /api/draft/replay` (script complet, réservé à la page). Aucun nouvel event :
`draft.updated` suffit. Mode ouvert (comme les routes draft existantes).

### Frontend

Nouvelle route `/draft/replay` (React, tokens existants) : zone de dépôt, liste de la file (partie en
cours en surbrillance), aperçu **non spoilant** de la séquence (étapes numérotées, héros masqués
jusqu'à révélation), contrôles Suivant/Retour/Reset/Play, bascule fearless, bouton « partie suivante ».
`/draft/overlay` : **aucun changement attendu** (il lit déjà `assignments`, pseudos, series bans).

## Hors périmètre

Import de la partie en base depuis cette page, replays sans draft, édition du script, talents,
multi-opérateur, auto-détection des noms d'équipe, overlay dédié « replay ».

## Critères d'acceptation

1. Un replay de draft réel (3 replays de l'opérateur) se charge en < 500 ms ; l'overlay reste **vide**
   jusqu'au premier « Suivant », puis se remplit dans l'ordre exact du draft réel (bans et picks
   alternés conformes à la fiche de match).
2. Retour/Reset/lecture auto fonctionnent ; un rafraîchissement de la page ou de l'overlay ne perd rien.
3. File de 3 replays en fearless : en partie 2, les picks de la partie 1 sont indisponibles ; les
   pseudos de joueurs sont affichés sur les tuiles de pick.
4. Un replay QM/ARAM est refusé avec un message clair ; aucune partie n'est ajoutée en base.
5. Tests : moteur (reveal/skip/undo/fearless), décodage sur mini-corpus, ordre global = `turn` du parser.

## Docs à mettre à jour avec le code (Règle n° 0)

`docs/spec/05-api.md` (routes), `04-serveur.md` (état/persistance, file), `08-frontend.md` (page
`/draft/replay`), `07-evenements.md` (inchangé : `draft.updated` réutilisé), `docs/STATUS.md`.
