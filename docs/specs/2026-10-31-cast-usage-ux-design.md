# Régie de cast — ergonomie d'utilisation (URL overlay, parcours, mise à l'antenne)

> Spec storm-codex, **validée par l'opérateur le 2026-10-31 (choix recommandés : C = bouton explicite, D = horloge serveur)**. Langue : anglais conservé (F, non tranché explicitement — à confirmer). Règle n° 0.
> Suite de `2026-10-25-cast-control-room-design.md` (régie livrée, PR #5 mergée sur `main`).
> Les points marqués **[choix Claude]** sont des propositions à relire ; ceux marqués **[à trancher]**
> remettent en cause une décision déjà actée et exigent une réponse explicite de l'opérateur.

## But

Un streamer qui découvre la régie doit pouvoir **sans rien chercher** : récupérer l'URL de la source
OBS, savoir dans quel ordre agir, voir ce qui est réellement à l'antenne, et comprendre un message
d'erreur. Aujourd'hui l'URL de l'overlay n'apparaît que sur l'onglet Draft (dans la couverture de la
console Draft) ; les onglets Series, Lineups et In-game ne la montrent pas.

## Constats (relecture du 2026-10-31, code de `main`)

1. **URL de l'overlay cachée** : `web/src/cast/DraftWorkspace.tsx` (l.74 et l.89) est le seul endroit
   qui affiche `/cast/overlay?skin=nexus`. `/cast` (lien « Cast » du `Layout`) ne l'affiche nulle part.
   L'étape 3 de l'assistant de 1er lancement (`components/Onboarding.tsx`) liste d'anciennes sources
   (`/queue`, `/ticker`, `/widget`, `/now-playing`) et **pas** `/cast/overlay`.
2. **Aperçu ≠ sortie OBS** : `cast/SeriesWorkspace.tsx` et `castState.ts` (l.242–249) l'avouent :
   en rotation automatique, l'aperçu et la source OBS ont chacun leur minuterie. Le critère 2 de la
   spec d'origine (« l'aperçu de la régie = ce que voit OBS ») n'est donc pas tenu en mode auto.
3. **Mise à l'antenne au clic** : un clic sur un onglet de scène envoie la scène à l'antenne
   (`POST /api/cast/scene`, R26). Pas de confirmation, et l'onglet de travail local peut différer de
   la scène à l'antenne sans que ce soit évident.
4. **Erreurs brutes** : `Server refused: <code>` affiche `NotInPool`, `SeriesInactive`,
   `MapAlreadyPlayed`, `side_mismatch: the replay's winner contradicts…`.
5. **Pas de parcours** : rien n'indique l'ordre (créer la série → équipes → pool → parties → scènes).
   « New series » est désactivé tant que le pool est vide, sans dire pourquoi ; « Save teams » est
   distinct de « New series », qui envoie quand même les équipes non sauvées.
6. **Jargon** : « BO 5 · GAME 2 · FEARLESS · score from game winners », modes Lineup « Auto / A / B »,
   « linked / unlinked », « awaiting game data », « Swap sides » (explication seulement en infobulle),
   « Custom replay » requis pour le post-game (seulement dans une consigne courte).
7. **Console Draft en doublon** : la couverture « Draft simulator · operator console » (grand titre)
   s'affiche dans la régie, avec un style qui ne ressemble pas au reste de `/cast`.

## Décisions proposées

### A. Bloc « Source OBS » permanent (constat 1, 7)

- Dans l'**en-tête** de `/cast`, toujours visible quelle que soit la scène : l'URL complète
  (`{origine}/cast/overlay?skin=…`), un bouton **Copier**, un sélecteur de skin
  (`nexus · glass · tactical · mono`, défaut `nexus`), et la consigne « Source navigateur · 1920×1080 ·
  fond transparent ». **[choix Claude]**
- L'URL se met à jour avec le skin choisi ; le skin est mémorisé côté navigateur uniquement
  (préférence d'affichage, pas d'état partagé). **[choix Claude]**
- Retirer la couverture « operator console » de l'onglet Draft ; garder dans l'onglet Draft seulement
  le lien « Draft depuis un replay ». **[choix Claude]**
- Étape 3 de l'assistant de 1er lancement : ajouter `/cast/overlay` à la liste, et la présenter comme
  la source principale de la régie (les autres restent optionnelles). **[choix Claude]**

### B. Parcours guidé (constat 5)

- Une **barre d'étapes** en tête de rail : *1 Série* (Bo, format, pool) → *2 Équipes & joueurs* →
  *3 Parties* → *4 En direct*. Chaque étape affiche son état (faite / à faire) à partir de l'état
  serveur, sans case à cocher à maintenir à la main. **[choix Claude]**
- Quand aucune série n'est active : le rail s'ouvre sur un court texte d'accueil (« Créez une série
  pour commencer ») et sur la seule action possible. **[choix Claude]**
- « New series » indique la raison d'être désactivé (« Cochez au moins une carte du pool »). **[choix Claude]**
- Une équipe modifiée non sauvée est signalée dans « New series » (« les équipes non sauvées seront
  envoyées » ou « Sauver les équipes d'abord ») ; aucune sauvegarde implicite silencieuse. **[choix Claude]**

### C. Mise à l'antenne explicite (constat 3) — **[à trancher]**

Le R26 a choisi « clic = à l'antenne ». Cette spec propose de le reconsidérer :

- **Option 1 (recommandée par Claude)** : un clic sur un onglet ouvre la scène **dans l'espace de
  travail seulement** ; un bouton **« Mettre à l'antenne »** bien visible (et « Entrée ») la diffuse.
  L'onglet à l'antenne reste marqué *ON AIR*, et l'onglet de travail non diffusé est signalé.
- **Option 2** : garder le clic direct, mais demander une confirmation quand la scène quittée est
  *In-game* avec une partie en cours.

Choix de l'opérateur requis : option 1 ou option 2 ? (Le clavier et les rôles ARIA restent ceux
décrits par la spec R26 dans les deux cas.)

### D. L'aperçu montre ce qu'OBS montre (constat 2) — **[choix Claude]**

- Recommandé : la **rotation automatique est pilotée par le serveur** (le serveur porte l'index de la
  slide et l'horodatage de son dernier changement ; l'overlay et l'aperçu calculent l'instant courant
  à partir de ces deux valeurs). Aperçu et OBS montrent alors la même slide, sans dépendre de deux
  minuteries locales.
- Alternative moins invasive : en mode auto, l'aperçu affiche **la slide reçue du serveur** et la
  régie n'affiche plus « rotating n slides ». Moins fidèle, mais sans changement serveur.
- Le choix impacte `sync`/`cast.updated` ; il doit être tranché avant le plan.

### E. Erreurs compréhensibles (constat 4) — **[choix Claude]**

- Une table `code → message en clair` (ex. `NotInPool` → « Cette carte n'est pas dans le pool de la
  série. », `SeriesInactive` → « Aucune série en cours. », `MapAlreadyPlayed` → « Cette carte a déjà été
  jouée. ») dans `castState.ts`, à côté de `replayErrorText` qui existe déjà.
- Code technique conservé en repli (infobulle ou ligne discrète), pour le support. Le bandeau reste
  « jamais avalé » comme aujourd'hui.

### F. Libellés et langue (constat 6)

- Libellés remplacés par des formulations courtes : « Lineup mode » → « Cartes d'équipe : avant la
  série / entre les parties / automatique ». Pastille : « BO5 · Partie 2 · Fearless » sans « score from
  game winners » (déplacé dans une infobulle).
- « Swap sides » : libellé « Inverser les côtés » avec consigne visible sous la case quand un replay
  est refusé avec `side_mismatch`.
- « Custom replay » : mention visible dans l'onglet Series quand aucune partie n'a de replay.
- **[à trancher]** Langue de l'interface : la spec d'origine a choisi des libellés en anglais
  (« convention de l'app »). Un streamer francophone gagnerait à une interface en français. Décision
  de l'opérateur requise ; ce document ne la prend pas.

## Hors périmètre

- Nouvelle fonctionnalité d'overlay, nouveau skin, nouvelle scène.
- Pilotage d'OBS (WebSocket OBS) — toujours exclu par la spec d'origine.
- Refonte visuelle complète de la régie (seuls le bloc OBS, le parcours et les libellés changent).
- Changement de contrat de données autre que celui décrit en D (si D est retenu).

## Critères d'acceptation

1. Depuis `/cast`, sans navigation, l'URL de la source OBS est visible et copiable en un clic, dans
   **tous** les onglets de scène, avec le skin choisi.
2. Depuis l'assistant de 1er lancement, `/cast/overlay` est listé et mène à la régie.
3. Un utilisateur qui n'a jamais utilisé la régie peut, en suivant seulement la barre d'étapes et les
   messages affichés, créer une série, la saisir et la mettre à l'antenne (test de parcours manuel
   consigné dans `docs/STATUS.md`).
4. Selon l'option choisie en C : aucune scène n'est diffusée sans action explicite (option 1), ou une
   confirmation est demandée quand une partie est en cours (option 2).
5. Selon D : en mode auto, l'aperçu et la source OBS affichent la même slide au même instant (mesuré
   sur deux onglets, écart ≤ 1 s), ou la régie n'affiche plus de promesse d'alignement (alternative).
6. Chaque code d'erreur listé en E affiche le message en clair ; un code inconnu affiche le code brut.
7. Aucune régression : les tests existants (`web` et serveur) restent verts ; `replayErrorText` et les
   garanties R19–R26 sont inchangés, sauf décision écrite contraire.

## Docs à mettre à jour avec le code (Règle n° 0)

- `docs/spec/08-frontend.md` : `/cast` (en-tête OBS, barre d'étapes, mise à l'antenne), `/cast/overlay`
  (si D change le fonctionnement), libellés.
- `docs/spec/05-api.md` et `07-evenements.md` seulement si D est retenu (horloge serveur de slide).
- `docs/STATUS.md` : mettre à jour l'état de la régie (la section « non mergée » est obsolète depuis PR #5)
  et le résultat du test de parcours (critère 3).

## Questions à trancher par l'opérateur

1. Mise à l'antenne : option 1 (bouton explicite, recommandé) ou option 2 (confirmation) — **[C]**.
2. Rotation auto : horloge serveur (recommandé) ou aperçu qui suit le serveur sans horloge partagée — **[D]**.
3. Langue de l'interface : français ou anglais — **[F]**.
4. Le bloc OBS doit-il être aussi visible sur mobile/petit écran, ou seulement en poste de régie ?
