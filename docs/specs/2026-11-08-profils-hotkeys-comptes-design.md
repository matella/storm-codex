# Profil de raccourcis dans les replays → regroupement des comptes d'une même personne — design

> Statut : **validée par l'opérateur le 2026-11-08** (source = replay, pas l'uploader ; `0_default` et
> noms ≤ 2 caractères ignorés, profil partagé = HAUT seul, seuil générique 5 comptes). Seuil 5 à
> régler sur une sonde du corpus réel. Complète `2026-10-25-recherche-joueurs-design.md` (comptes liés).

## Intention

Les replays ne relient jamais deux comptes directement, mais chaque client écrit **le nom du profil de
raccourcis clavier du joueur** dans le replay. Ce nom est choisi à la main (`Kure1`, `munky`…) : le
retrouver sur deux comptes est un indice très fort de « même personne », et il désigne la personne.

## Constat (vérifié le 2026-11-08 sur un vrai replay)

- Le flux d'événements de jeu (`--stream game`) contient, à `_gameloop` 0, un événement
  `NNet.Game.SUserOptionsEvent` **par joueur** avec `m_hotkeyProfile`. Valeurs vues dans un seul
  replay : `PrizraK`, `samuro`, `0_default`, un nom cyrillique. C'est donc le **profil actif de chaque
  joueur de la partie**, et non seulement celui de l'opérateur.
- Rattachement à un compte : `_userid.m_userId` → `toon_handle` par le mapping `playerLobbyID` que
  `storm-stats` construit déjà (`process/mod.rs`, autour de la ligne 481).
- Rien n'est extrait aujourd'hui : aucune occurrence de `hotkeyProfile` dans `crates/` ni `web/`.
- Conséquence : **pas besoin de l'uploader ni d'un accès au PC**. Tout le corpus déjà archivé (re-process
  idempotent via `parser_version`) donne le signal, pour tous les joueurs rencontrés.
- Côté PC de l'opérateur, `Variables.txt` (`hotkeyprofile=`) vaut `English` partout : inutile ici, on
  ne s'en sert pas.

## Décisions

1. **Extraction** : lors du traitement d'un replay, lire `SUserOptionsEvent.m_hotkeyProfile` pour chaque
   joueur (hors observateurs) et le stocker par `(match, toon)`. Pas de nouvelle source de données.
2. **Noms ignorés par défaut** : `Standard`, `Standard for Lefties`, les noms de langues (anglais et
   forme native : English, Français, Deutsch, Español, Русский, 한국어, 中文…), et `0_default` (valeur
   réelle vue dans les replays). Comparaison insensible à la casse, aux espaces de bord. Liste
   surchargeable via `app_settings` (`ignored_profile_names`). **[mon choix]** les noms ≤ 2 caractères
   sont ignorés aussi.
3. **Nom générique automatique** : un nom porté par trop de comptes distincts n'identifie plus
   personne (ex. `samuro`, nom de héros). **[mon choix]** seuil initial : ≥ 5 comptes distincts, **à
   régler sur une sonde du corpus réel** avant de figer (même démarche que `sonde_identite_corpus_reel`).
4. **Signal « profil partagé »** : deux comptes ayant porté, dans au moins une partie chacun, un même
   nom non ignoré. Force : **forte**. **[mon choix]** il suffit seul à atteindre le niveau HAUT, tout en
   restant soumis à l'exclusion temporelle (deux comptes dans la même partie ≠ même personne).
5. **Historique de profils par compte** : on garde les noms de profil par compte avec dates
   (première/dernière partie), comme l'historique des pseudos. Un changement de profil sans nouveau
   pseudo reste lisible.
6. **Affichage** : dans l'onglet Identité, une raison « Profil partagé `munky` » dans les suggestions et
   une ligne « Profils utilisés » dans la fiche. Un lien reste une simple note (pas de fusion de stats).

## Historique des pseudos d'un compte (demande liée)

Le `toon_handle` est stable, le pseudo change. **Déjà en place** : `players::names::name_history`
(pseudos datés, récent d'abord) exposé par `GET /api/players/{toon}/identity` et affiché dans
l'onglet Identité (`PlayerIdentity.tsx`, « show all N »). Rien de nouveau à construire côté données ;
si l'opérateur veut le voir ailleurs (en-tête de profil, résultats de recherche), c'est un ajout
d'affichage à préciser séparément.

## Données et API (à détailler au plan)

- Stockage du profil par `(match_id, toon_handle)` : colonne dédiée sur `match_players` ou champ de
  `data` ; **choix laissé au plan** après mesure du coût de la requête de groupement.
- `parser_version` incrémentée pour déclencher le re-process du corpus.
- `similarity.rs` : signal `profile` ; `identity.rs` : présélection ajoute les comptes partageant un nom
  rare (chemin prioritaire avec les co-propriétaires de token).
- Docs à mettre à jour dans le même commit : `docs/spec/05-api.md`, la spec schéma/projection,
  `08-frontend.md`.

## Hors périmètre

- Contenu des raccourcis (non présent dans le replay, et inutile).
- Toute identité civile : le nom de profil est un pseudo choisi par le joueur.
- Modification de `client-rs` (devenue inutile).

## Questions ouvertes pour l'opérateur

1. Noms ignorés supplémentaires : `0_default` (vu dans les replays) et noms ≤ 2 caractères ?
2. Un profil partagé seul donne-t-il HAUT, ou faut-il un second signal ?
3. Seuil de nom générique : 5 comptes à confirmer par une sonde sur le corpus.
