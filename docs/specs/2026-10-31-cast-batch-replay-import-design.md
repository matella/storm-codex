# Import groupé des replays d'une série de cast — design

**Statut** : validée par l'opérateur (2026-10-31) — décisions 1 à 3 ci-dessous tranchées.
**Date** : 2026-10-31
**Domaine** : cast (`crates/storm-codex-server/src/cast/`, `web/src/cast/`), spec `docs/spec/05-api.md` §Replay d'une partie.

## Contexte

Aujourd'hui, le replay Custom de chaque partie se dépose une fois la partie lancée et associée à sa ligne
de série (`POST /api/cast/games/{n}/replay`, un fichier par partie). Pour une série de 3 à 5 parties, c'est
un aller-retour par partie, après coup.

## Objectif

Au moment de créer une série, proposer à l'opérateur :

- **Manuel** (défaut) : comportement actuel, un fichier par partie.
- **Groupé** : déposer d'un coup les `.StormReplay` de la série ; le serveur propose l'affectation
  partie ↔ fichier ; l'opérateur confirme ou corrige avant toute écriture.

## Hors périmètre

- Pas de lecture de dossier côté serveur (le navigateur envoie les fichiers, comme aujourd'hui).
- Pas d'écriture en base (`matches`, `uploads`) : seule la ligne `cast_live` change, comme la route actuelle.
- Pas de changement de la règle d'orientation (R14/R16) ni du format de `GameSummary`.

## Points à vérifier avant implémentation

1. **Carte** : `GameSummary.map` existe (`cast/mod.rs`, champ `map`). Sert à rattacher un replay à une carte du pool.
2. **Date / ordre** : le résumé ne porte pas, à ce jour, la date de la partie. À vérifier dans `storm-stats`
   (`process_replay`) ; sinon, l'ordre se fait par défaut sur l'ordre de dépôt, à confirmer par l'opérateur.
3. **Création des parties** : une partie n'existe qu'après `POST /api/cast/games/start` (carte choisie dans le pool).
   Le groupé doit donc soit créer les parties à partir des replays (carte du replay ∈ carte choisie du pool),
   soit ne rattacher qu'aux parties déjà lancées. **Choix proposé** : créer les parties (vue complète d'emblée).

## Comportement proposé

1. `POST /api/cast/series` inchangé ; l'écran de création ajoute le choix Manuel / Groupé.
2. Mode Groupé, après l'étape équipes :
   - `POST /api/cast/series/replays/preview` (corps multipart ou liste d'octets + noms) : décode chaque fichier
     (même chemin que `cast/replay.rs::decode`, fichiers temporaires supprimés), renvoie pour chacun
     `{ filename, map, winner, orientation, proposed_game }` sans rien écrire. `proposed_game` = la carte du
     pool (statut `picked`/`decider`) correspondante, dans l'ordre de dépôt à défaut de date.
   - Erreurs par fichier (`not_custom`, `parse_failed`, `incomplete`) affichées sans bloquer les autres.
3. L'opérateur corrige l'affectation dans l'écran, puis valide.
4. `POST /api/cast/series/replays/commit` : pour chaque affectation validée, crée la partie si besoin puis applique
   `apply_to` (même verrou, même `series_changed`/`side_mismatch`). Réponse : résultat par fichier.
   Atomicité : si une affectation échoue, les précédentes restent (chaque partie est indépendante) ; le résultat
   le dit explicitement.
5. `cast.updated` diffusé comme aujourd'hui ; synchro draft inchangée.

## Contraintes reprises de la spec

- Décodage sous `parse_sem`, fichier temporaire garanti supprimé (garde `TempFile`).
- Budget : ~180 ms par replay en release → 5 replays < 1 s, à mesurer (règle dure 4, pas d'estimation).
- Corps limité comme les autres routes de replay ; pas de nouvelle limite implicite.

## Tests attendus

- Preview : affectation par carte ; fichier non Custom signalé sans bloquer les autres ; rien écrit en base.
- Commit : partie créée puis remplie ; `series_changed` si la série change entre preview et commit ;
  `side_mismatch` remonté par fichier.
- Front : choix Manuel/Groupé à la création ; correction manuelle de l'affectation ; Manuel inchangé (non-régression
  des tests `SeriesWorkspace` existants).

## Décisions (validées)

1. **Créer les parties depuis les replays** : chaque fichier crée sa partie (carte du pool `picked`/`decider`, non encore jouée).
2. **Ordre** : date de partie si le replay la fournit, sinon ordre de dépôt.
3. **Commit partiel** accepté : chaque partie réussit ou échoue seule, le résultat est renvoyé par fichier.
