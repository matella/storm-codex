# Simulateur de draft HotS + overlays — design

> Spec storm-codex. Brainstorming validé le 2026-06-19. Process léger (décisions batchées,
> spec courte, pas de boucle de revue subagent). Cible : `writing-plans` puis implémentation TDD.

## But

Un simulateur de draft Heroes of the Storm **piloté 100 % manuellement** par l'opérateur
(tous les choix des deux équipes), avec **trois formats** de draft et **deux overlays OBS** en
direct (vue joueur + vue caster) reflétant l'état du draft façon écran de draft en jeu.

Pas de bot/IA : l'opérateur fait chaque ban et chaque pick des deux côtés.

## Décisions actées (brainstorming)

1. **Sync** : état de draft **autoritatif côté serveur** + broadcast WebSocket. Une console de
   contrôle pilote ; les overlays s'abonnent en direct (vrai second écran, résiste au refresh).
2. **Fearless** : **suivi de série complet** (auto-accumulation des bans des parties précédentes)
   **+** override manuel de la disponibilité de n'importe quel héros.
3. **Vue caster** : noms d'équipe **et** score éditables (en plus de la vue joueur).
4. **3 formats dès la V1** : Standard, Normal, Fearless.

## Idée-clé : le format = une liste d'étapes (data), pas du code

Le moteur est un **marcheur générique** sur une liste ordonnée d'étapes :

```
Step = { team: Team1 | Team2, action: Ban | Pick }
DraftFormat = { id, label, steps: Step[], prebans: bool, fearless: bool }
```

Chaque format n'est qu'une `steps[]` différente. Corriger l'ordre exact = éditer une liste,
jamais le moteur. Les séquences vivent dans un module de constantes éditable.

### Séquences proposées (À CONFIRMER par l'opérateur — il est l'expert HotS)

**Standard** (séquence tournoi des captures : 3 bans/équipe, 2 phases de bans, picks 1-2-2-…-1-2-1) :

| # | Équipe | Action | | # | Équipe | Action |
|---|--------|--------|---|---|--------|--------|
| 1 | T1 | Ban | | 9 | T1 | Pick |
| 2 | T2 | Ban | | 10 | T2 | Pick |
| 3 | T1 | Ban | | 11 | T1 | **Ban** (2e phase) |
| 4 | T2 | Ban | | 12 | T2 | **Ban** (2e phase) |
| 5 | T1 | Pick | | 13 | T2 | Pick |
| 6 | T2 | Pick | | 14 | T1 | Pick |
| 7 | T2 | Pick | | 15 | T1 | Pick |
| 8 | T1 | Pick | | 16 | T2 | Pick |

→ T1 : 3 bans + 5 picks. T2 : 3 bans + 5 picks. Total 6 bans / 10 picks.

**Normal** = Standard **sans les étapes de ban** (steps 1-4, 11-12 retirées) → pur ordre de pick
1-2-2-…-1-2-1, aucun ban. *(« pas de préban » = pas de bans du tout ; à confirmer.)*

**Fearless** = séquence Standard, mais l'ensemble « indisponible » est **pré-rempli** avec tous
les héros pické aux parties **précédentes** de la série courante. Les bans/picks de la partie en
cours s'ajoutent normalement.

T1 = équipe « first pick » ; l'opérateur choisit quelle équipe (bleue) est first-pick et peut
inverser les côtés.

## Architecture (storm-codex : axum + sqlx + Postgres + WS `/ws` existant)

### Moteur de draft — Rust pur, TDD, zéro I/O

`server/src/draft/engine.rs` (ou crate/module dédié) :

- `DraftState { format, map, team_names, score, first_pick, steps, cursor, bans[T1/T2],
  picks[T1/T2], unavailable: Set<HeroKey> }`
- `apply_action(state, hero) -> Result<DraftState, DraftError>` : assigne `hero` à l'étape
  courante (`steps[cursor]`), avance le curseur. Refuse si héros indisponible (déjà pick/ban,
  fearless, ou override manuel) → `DraftError::Unavailable`. Refuse si draft terminé.
- `undo(state) -> DraftState` : recule le curseur, libère le dernier héros.
- `reset(state) -> DraftState` : repart à `cursor=0`, conserve format/map/équipes/série.
- `toggle_unavailable(state, hero)` : override manuel (ajoute/retire de `unavailable`).
- `is_complete(state) -> bool`, `current_step(state) -> Option<Step>`.
- Légalité : un héros = au plus une occurrence sur l'ensemble bans+picks+unavailable.

**Tests** (le cœur de la TDD) : marche complète des 3 séquences ; refus de héros indisponible ;
undo/redo de chaque type d'étape ; fearless pré-rempli cumulatif sur 2-3 parties ; override
manuel ; cas limites (undo sur draft vide, pick sur draft terminé).

### État serveur + persistance

- **Singleton « draft courant »** en mémoire (un seul opérateur) + persisté pour survivre au
  redémarrage. Table `draft_live` (1 ligne : JSON de l'état + `updated_at`).
- **Série** (pour fearless) : table `draft_series { id, format, bo, games: jsonb[], score,
  team_names, created_at }`. Une partie terminée pousse ses picks dans `games`. « Partie suivante »
  crée un nouveau draft live pré-rempli depuis l'historique ; « nouvelle série » réinitialise.
- Versionné : chaque état porte un `schema_version` (cohérent avec les conventions du repo).

### Routes HTTP

- `GET  /api/draft` → état courant.
- `POST /api/draft/config` → `{ format, map, first_pick, team_names, bo }` (réinitialise le draft).
- `POST /api/draft/action` → `{ hero }` (assigne à l'étape courante).
- `POST /api/draft/undo`, `POST /api/draft/reset`.
- `POST /api/draft/unavailable` → `{ hero, value: bool }` (override manuel).
- `POST /api/draft/score` → `{ team1, team2 }`.
- `POST /api/draft/series/next` (partie suivante, pré-remplit fearless),
  `POST /api/draft/series/new` (nouvelle série).

### WebSocket

Étendre le broadcast `/ws` existant avec un event `{ type: "draft.updated" }`. Les pages (console
+ overlay) réagissent en invalidant/refetchant `GET /api/draft` (même pattern que `match.parsed`).

## Frontend (React/Vite, react-router) — 2 routes

Réutilise `/api/dim/heroes` (roster + `role` + `universe` + `icon`), `/images/heroes/*`,
`/images/battlegrounds/*`, le composant `Avatar`, le thème.

Le visuel est **figé** dans deux mockups livrés avec la spec :
- `draft-aesthetics.html` — l'overlay (vue **Timeline**, 4 skins swappables, vue Caster en réf.).
- `draft-control-mockup.html` — la console de contrôle.

### `/draft` — console de contrôle (écran opérateur)

- **Barre de config** : **format** (Standard/Normal/Fearless), **map**, équipe **first-pick**
  (Blue/Red), **Bo** (1/3/5/7), **score** (steppers). Actions : **Undo**, **Reset**,
  **Partie suivante**, **Nouvelle série**.
- **Bandeau de phase courante** : « Au tour de Blue — Pick (4ᵉ choix) » (équipe + action +
  index) + le **timer de phase** (barre + compte à rebours).
- **Series bans (fearless)** : ruban des héros indisponibles (déjà pické dans la série) + compteur.
- **Colonnes Blue / Red** : **nom d'équipe éditable**, bans, 5 slots de pick chacun avec **champ
  pseudo éditable** ; le slot/ban courant est surligné (couleur d'équipe).
- **Picker central** : recherche texte + **onglets de rôle** (Tous, Tank, Bruiser, Assassin M./D.,
  Soigneur, Soutien) + **grille de héros** ; indispo grisés (« indispo »), hover = surbrillance.
  Clic → `action` (assigne à l'étape courante, avance). Clic-droit/long → toggle override manuel.

### `/draft/overlay` — overlay OBS (browser source 1920×1080, fond transparent)

**Vue Timeline** (unique overlay livré ; la perspective joueur = l'écran du jeu lui-même).
Paramètre `?skin=nexus|glass|tactical|mono` (défaut **nexus**). Composition :
- **En-tête** : titre map, **noms d'équipe** + **point clignotant sur l'équipe active**, **score**,
  **timer de phase** (barre qui se vide).
- **Series bans** (fearless) : ruban de mini-icônes barrées du pool indisponible.
- **Picks dans l'ordre chronologique** : grille de colonnes (1 par étape de pick), **bleu en haut /
  rouge en bas** d'une ligne invisible, colonnes vides = respirations ; tuiles **portrait** avec
  **héros + pseudo** écrits dessus ; tuile active = halo or pulsant.
- **Bans de la game** : bande centrale de petites tuiles barrées sur la ligne invisible, en chrono.

Les 4 skins = jeux de variables CSS (couleurs, rayon, clip-path, polices). Polices web
(Oswald, JetBrains Mono, Cinzel) chargées en prod.

### Détails tranchés

- **Map** = cosmétique (fond + titre), n'affecte pas les règles.
- **Timer = par phase** : une phase = un bloc d'actions consécutives d'une même équipe (1 ban,
  1 pick, 2 picks…). Le compte à rebours court jusqu'au passage de la main à l'équipe adverse,
  pas par pick individuel. Piloté par l'opérateur (start/reset) — pas de contrainte de temps dure.

## Hors périmètre V1 (YAGNI)

Bot/IA de pick, suggestions Jarvis (passerait par un Intent plus tard), talents, multi-opérateur
simultané, contrainte de temps dure, partage public d'un draft, overlay « vue joueur » dédié
(couvert par l'écran du jeu).

## Critères d'acceptation

1. Le moteur joue les 3 séquences de bout en bout, refuse les héros indisponibles, undo/reset OK
   (tests verts).
2. Depuis `/draft`, un draft Standard complet se construit au clic ; `/draft/overlay` (Timeline)
   se met à jour **en direct** (WS) sur un 2e écran/OBS, et `?skin=` change l'esthétique.
3. Fearless : sur la partie 2 d'une série, les héros pické en partie 1 sont automatiquement
   indisponibles (ruban series bans) ; l'override manuel fonctionne.
4. Noms d'équipe + pseudos + score éditables depuis la console et reflétés dans l'overlay.
