# Régie de cast — design

> Spec storm-codex, **à valider par l'opérateur avant tout code** (Règle n° 0). Brainstorming du
> 2026-10-25 avec l'opérateur. S'appuie sur le simulateur de draft (`2026-06-19-draft-simulator-design.md`)
> et le draft depuis un replay (`2026-10-25-draft-replay-design.md`, mergé sur `main`).
> Les points marqués **[choix Claude]** sont des décisions prises sans l'opérateur : à relire.
> Maquettes : `docs/specs/2026-10-25-cast-mockups/` (fragments HTML, à ouvrir dans un navigateur ;
> les noms, scores et portraits y sont fictifs ; la capture de fond `ingame.webp` ne garde nette que
> la bande de HUD du haut — tout le reste, minimap et pseudos compris, est flouté).

## But

Donner au caster **une seule page d'administration** pour piloter tout ce que voit le public d'une
série (Bo3/Bo5) : veto des cartes, lineups, draft des héros, bandeau en partie, résultats entre les
parties. Aujourd'hui seul le draft des héros existe (`/draft`, `/draft/replay`, overlay
`/draft/overlay`) ; les cartes n'y sont que décoratives et rien ne couvre le reste d'une retransmission.

Inspiration : Soothsayer (ebshimizu, MIT) — un panneau de contrôle de caster + des overlays OBS.
**Réécriture complète** dans la pile Storm Codex et son design system : aucun code repris.

## Décisions actées (avec l'opérateur)

1. **Une régie `/cast`**, disposition « B » : colonne gauche permanente (série : équipes, lineups, Bo,
   Fearless, score, cartes), onglets de scène + **aperçu en direct** de ce qui est à l'antenne au
   centre, espace de travail de la scène sélectionnée en dessous (maquette `control-room-layout.html`).
2. **Cinq scènes** : *Lineups*, *Series*, *Draft*, *In-game*, *Hide*. Changer de scène change ce que
   montre OBS, sans rien toucher dans OBS.
3. **Deux sources de données, même affichage** : en direct, le caster saisit tout à la main (cartes,
   bans/picks de héros, Fearless) ; sur replay, le draft se remplit seul depuis le replay (déjà livré),
   et les résultats de partie aussi.
4. **Veto de cartes saisi, pas piloté** : les bans/picks de cartes se font ailleurs (capitaines) ; le
   caster les **reporte dans un ordre quelconque**. Aucune séquence imposée. Au démarrage de la
   série, il choisit **quelles cartes sont dans le pool** (≈ 10 sur les cartes du jeu).
5. **Format HotS standard** pour le draft de héros (Standard / Normal / Fearless existants).
6. **Style** : Nexus Codex — skin « nexus » de l'overlay de draft pour les scènes plein écran, look
   « dossier » de l'app pour la régie. Les 4 skins existants restent disponibles.

## Hors périmètre / anti-objectifs

- Lecture automatique du draft **depuis une partie en cours** (le caster saisit ; le replay est le seul mode auto).
- Pilotage d'OBS lui-même (changement de scènes OBS, WebSocket OBS) : la régie ne parle qu'à notre overlay.
- Taux de victoire ou statistiques issus de notre base affichés comme « niveau tournoi » : trompeurs
  (parties de tous modes), **donc exclus**. Les stats affichées sont soit saisies, soit tirées de
  la partie en cours de présentation.
- Multi-opérateur simultané, sponsors/tickers, chat, talents, bot de draft.
- Recouvrir l'interface native du jeu : l'overlay en partie ne doit rien cacher du HUD.
- **Spoilers** : en mode replay, rien du futur ne part vers l'overlay (règle déjà acquise).

## Modèle de série (nouveau)

Une **série** est l'entité centrale ; la régie en pilote une à la fois (singleton persistant, comme
`draft_live`). Contenu :

- **Équipes** (Blue / Red) : nom (obligatoire), logo (optionnel, image téléversée ou URL), 5 joueurs :
  BattleTag/pseudo, **ligne libre optionnelle** (rôle, slogan — saisie par le caster), lien vers le
  joueur de notre base si le pseudo est résolu. Un drapeau « capitaine » optionnel **[choix Claude —
  à retirer si inutile]**.
- **Format** : Bo (1/3/5/7), mode du draft (Standard / Normal / Fearless).
- **Score de série** : victoires par équipe (déduit des parties terminées, corrigeable à la main).
- **Pool de cartes** : liste choisie au démarrage. Chaque carte porte un statut :
  `disponible` · `bannie par X` · `choisie par X` · `decider`. Pas d'ordre imposé entre les saisies.
  Le decider est **marqué à la main** (ou proposé quand il ne reste qu'une carte **[choix Claude]**).
- **Parties** : numéro, carte jouée, vainqueur, score de takedowns, durée, référence au replay si chargé.
  La carte de la partie N est **choisie parmi les cartes « choisie »/decider au lancement de la partie**
  **[choix Claude]** (l'ordre de jeu n'est pas forcément l'ordre de pick).
- **Scène courante** + état de rotation des slides de la scène *Series*.

Le **draft de héros** reste porté par le moteur existant (`DraftState`). Invariant à respecter :
**noms d'équipes, joueurs, score et Bo n'ont qu'une seule source de vérité** (la série) — le draft les
lit ou y est synchronisé ; jamais deux endroits éditables. Le détail est laissé au plan.

## Scènes

### Lineups (cartes d'équipe) — maquette `team-cards.html`

Plein écran, skin nexus. Deux cartes (Blue gauche, Red droite) : logo, nom, record si saisi, 5 joueurs.
**Deux modes de la même carte** :
- **A — avant la série** (aucune partie jouée) : par joueur, ses **3 héros les plus joués sur ses
  20 dernières parties** de notre base, **masqués** s'il a moins de ~5 parties récentes **[choix
  Claude : seuils à relire]** ; plus la ligne libre saisie. **Pas de win rate** (décision opérateur).
- **B — après ≥ 1 partie** : les portraits deviennent **les héros réellement joués dans cette série**,
  dans l'ordre, ceux verrouillés par Fearless atténués.
Bascule automatique A→B à la première partie terminée ; **forçage manuel** possible dans les deux sens.

### Series (entre les parties) — maquettes `styled-mockups.html`, `postgame-breakdown.html`

- **Bandeau des cartes** (toujours visible) : toutes les cartes du pool ; pour chacune, qui l'a
  bannie/choisie, **« WON » dans la couleur de l'équipe gagnante** (pas de chiffres sur la carte),
  **« NEXT »** doré sur la prochaine, `decider`, bannies barrées.
- **Diaporama de fin de partie** sous le bandeau, alimenté par le **replay décodé de la partie qui
  vient de se jouer** : écran « post-game breakdown » (en-tête WIN/LOSS, score de takedowns, durée,
  carte ; colonne de comparatifs ; panneau de dégâts par joueur avec onglets ; courbe d'avantage
  d'expérience). Défilement **automatique** (durée réglable) ou **à la main** depuis la régie.
- Correspondance LoL → HotS : or → avantage d'expérience ; tourelles → forts/forteresses ; drakes/barons
  → camps de mercenaires et objectifs de carte.
- Données (déjà dans storm-stats) : dégâts héros, soins, dégâts de siège, takedowns/morts/assists,
  niveau d'équipe, forts/keeps, camps, objectifs de carte, XP périodique, bans. **Sans replay chargé**,
  seules la bande des cartes et le score s'affichent (le caster saisit le vainqueur) **[choix Claude]**.

### Draft

Le draft existant (live manuel + replay, Fearless, file de replays, timers) **déménage dans la
régie** : mêmes fonctions, sélecteur de héros et colonnes Blue/Red dans l'espace de travail. À parité
fonctionnelle, `/draft` et `/draft/replay` redirigent vers `/cast` ; `/draft/overlay` continue de
fonctionner tant que l'ancienne source OBS existe **[choix Claude]**.

### In-game — maquette `ingame-bar-v6.html` (finale : voir « rectangle » ci-dessous)

Fond transparent posé sur la capture du jeu. **Ne recouvre pas le HUD natif** (bandeau de portraits en
haut, panneau central niveau d'équipe / structures / chrono, minimap bas-droite, panneau de replay).
- **Deux onglets rectangulaires** sous les bandes de portraits, de part et d'autre du panneau central
  du jeu : nom d'équipe + **score de série**. Le coin supérieur interne de chaque onglet est
  **ancré** (≈ 39,2 % et 59,65 % de la largeur, y ≈ 9,9 % de la hauteur pour 1920×1080 ; mesuré sur
  les captures de l'opérateur) ; le nom **grandit vers l'extérieur** jusqu'à ≈ 26 % de la largeur,
  puis est tronqué par « … ». **Bords droits, coins carrés** (décision opérateur : pas d'angles
  imitant les `\` `/` du jeu).
- **Pastille en haut à droite** : `BO5`, numéro de partie, et `FEARLESS` (ou `STANDARD`/`NORMAL`)
  **[la carte jouée peut s'y ajouter — à trancher]**.
- Ces positions dépendent de l'interface du jeu en 1920×1080 ; les **ancres sont des constantes
  réglables** (décalage fin depuis la régie ou en CSS) pour absorber un changement d'UI.

### Hide

Overlay entièrement transparent.

## Overlay OBS

**Une seule browser source** `/cast/overlay` (1920×1080, fond transparent) qui **suit la scène
courante** par WebSocket (`cast.updated`) ; transition courte (fondu ≈ 300 ms **[choix Claude]**).
Rafraîchir la page ou la source ne perd aucun état.

## Régie `/cast`

- Colonne gauche : **Série** (équipes + score, Bo, mode), **Lineups** (5 joueurs par équipe, ligne
  libre, logo), **Cartes** (pool, statuts, parties).
- Barre de scènes avec indicateur « à l'antenne », aperçu en direct (l'overlay réel, mis à l'échelle).
- Espace de travail par scène : *Lineups* (mode A/B forcé), *Series* (diaporama : slide courante,
  auto/manuel, durée), *Draft* (existant), *In-game* (steppers de score, correction de la pastille),
  *Hide*.
- Libellés en anglais (convention de l'app) ; mode ouvert comme les routes draft actuelles, avec la
  même garde que le reste si l'opérateur le demande **[choix Claude]**.

## Contrats techniques (esquisse — le plan les précise)

- Serveur : nouveau module `cast` (état de série JSON persisté, singleton), routes `/api/cast/*`
  (lecture, série, équipes, pool/cartes, parties, scène, slides), event WS `cast.updated` (signal
  sans état, comme `live.updated`). Réutilise le moteur de draft et le décodage de replay existants.
- Lineups : requêtes de héros récents par joueur sur notre base (budget : < 100 ms pour 10 joueurs).
- Post-game : un endpoint qui sert à l'overlay le résumé d'une partie (depuis le replay décodé ou la
  projection si la partie est déjà en base) ; **un replay chargé pour caster n'est pas écrit en base**
  (règle déjà actée pour le draft depuis replay).
- Front : routes `/cast` (régie) et `/cast/overlay` ; composants d'overlay partagés avec
  `DraftOverlay` plutôt que dupliqués.

## Critères d'acceptation

1. Depuis `/cast`, on crée une série (2 équipes, 5 joueurs chacune, Bo5, Fearless, pool de 10 cartes),
   on reporte bans et picks de cartes dans un ordre quelconque, et la scène *Series* les reflète.
2. **Changement de scène → overlay à jour en < 500 ms** (mesuré, WS) ; l'aperçu de la régie = ce que voit OBS.
3. Les cartes d'équipe passent de A à B après la première partie, avec Fearless atténuant les héros
   déjà joués ; forçage manuel OK.
4. Un replay de partie terminée alimente le « post-game breakdown » (toutes les valeurs affichées
   concordent avec la fiche de match de la même partie) ; rien n'est écrit en base.
5. L'onglet en partie ne recouvre aucun élément du HUD sur une capture réelle 1920×1080 ; avec des noms
   de 40 caractères, le nom est tronqué sans déborder.
6. Le draft (live et replay, Fearless, file) fonctionne depuis la régie avec les mêmes garanties
   qu'avant (tests existants verts) ; aucune donnée d'équipe n'est éditable à deux endroits.
7. Rafraîchir la régie ou l'overlay ne perd aucun état ; redémarrage du serveur idem.

## Docs à mettre à jour avec le code (Règle n° 0)

`docs/spec/05-api.md` (routes `/api/cast/*`), `04-serveur.md` (module `cast`, persistance),
`06-modele-donnees.md` (table de série), `07-evenements.md` (`cast.updated`), `08-frontend.md`
(`/cast`, `/cast/overlay`, redirections de `/draft*`), `docs/STATUS.md`.

## Précisions issues de la relecture (revue du 2026-10-25)

- **Source de vérité des équipes [choix Claude]** : la série est **autoritaire** ; `DraftState` **garde
  ses champs** (`blue`/`red`/`score`/`bo`, pas de migration du JSON `draft_live`, anciens tests et
  `/draft/overlay` intacts) et reçoit une **synchronisation à sens unique** série → draft à chaque
  modification. Dès qu'une série est active, l'édition de ces champs depuis l'ancienne page `/draft`
  est désactivée (lecture seule) ; elle disparaît avec la redirection vers `/cast`.
- **Joueurs ↔ notre base [choix Claude]** : le caster tape un pseudo ; la régie utilise la recherche
  existante `GET /api/players/search` (anciens pseudos inclus) et propose un **sélecteur** quand il y a
  plusieurs résultats ; aucun résultat = joueur « non lié » (ligne libre seule, pas de héros récents).
  Jamais de premier résultat choisi en silence. « 20 dernières parties » = **tous modes confondus**
  (il s'agit de héros joués, pas de niveau).
- **Post-game et replays [choix Claude]** : le caster **dépose un replay par partie** dans la régie
  (même chemin que la file du draft depuis replay, Custom uniquement). Un **résumé compact de la
  partie** (héros par joueur, valeurs du breakdown, séries pour la courbe) est **conservé dans l'état
  de la série** — pas de re-décodage, rien en base. Le caster peut rouvrir le breakdown de **n'importe
  quelle partie** de la série ; Fearless et le mode B de la carte d'équipe lisent ces mêmes résumés.
- **Données du breakdown** : la **parité avec la fiche de match** est le critère ; tout champ affiché
  doit exister dans la sortie actuelle de storm-stats pour un replay (dégâts héros, soins, dégâts de
  siège, takedowns/morts/assists, niveau d'équipe, forts/keeps, camps, objectifs de carte, XP
  périodique). **Pas de nouvelle métrique** dans storm-stats pour cette fonctionnalité : une valeur
  absente est retirée de l'écran, pas inventée. Le plan vérifie champ par champ avant de figer l'écran.
- **Logos [choix Claude]** : téléversement d'une image (PNG/WebP/JPEG, **≤ 512 Ko**) stockée en base
  et servie par le serveur, **ou** URL. Pas de stockage fichier sur le box.
- **Accès [choix Claude]** : lecture ouverte (l'overlay OBS n'a pas d'identifiants) ; **écritures de
  `/api/cast/*` ouvertes comme les routes draft** tant que le serveur n'est joignable que par
  Tailscale/LAN. Si l'opérateur l'expose, ajouter la même garde que `/api/admin`.
- **Pool de cartes** : la liste vient du référentiel existant (cartes + visuels déjà servis pour
  l'overlay de draft et `/maps`) ; le caster coche les cartes du pool au démarrage de la série.

## Laissé au plan (suggestion de découpage, non contraignante)

1. Modèle de série + régie (coque, scènes, aperçu) + overlay en partie + pastille.
2. Pool de cartes, veto, parties, scène *Series* (bandeau).
3. Cartes d'équipe (modes A/B).
4. Post-game breakdown et diaporama.
5. Migration du draft dans la régie et redirections.
