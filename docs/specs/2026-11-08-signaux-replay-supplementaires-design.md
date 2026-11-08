# Signaux supplémentaires tirés des replays — design

> Statut : **validée par l'opérateur le 2026-11-08** (ordre chat → déserteurs → cosmétiques → niveau →
> clavier). Plan du chat : `docs/plans/2026-11-08-chat-habitudes.md`. Les autres lots auront leur plan
> avant d'être codés. Fait suite à `2026-11-08-profils-hotkeys-comptes-design.md`.

## Intention

Six idées ont été proposées après le profil de raccourcis. Une sonde sur de vrais replays (40 replays
tirés au hasard de `Documents/Heroes of the Storm`, plus le corpus de la box : 34 600 lignes
`match_players`, 3 452 parties avec messages) dit lesquelles tiennent. L'opérateur les veut toutes ;
le chat se limite à du **volume et des habitudes**, sans jugement de toxicité, affiché sur la fiche
joueur et dans le brief de scouting.

## Verdict de la sonde

| # | Idée | Faisable ? | Constat chiffré |
|---|---|---|---|
| 1 | Cosmétiques équipés | **Oui** | skin/mount/announcer déjà dans `match_players.data` : 1 628 skins, 555 montures, 70 annonceurs distincts. Banner, spray, voiceLine, clanTag : vides dans les replays récents ou souvent vides |
| 2 | Groupes (premade) | **Non** | `m_tandemLeaderUserId` = `null` sur 636 slots, `m_teamsTogether` = `false` sur 40/40. La contrainte « pas d'info de groupe » de la spec joueurs reste vraie |
| 3 | Empreinte clavier | **Oui** | `STriggerKeyPressedEvent` : ~695 000 événements sur 40 replays (~17 000/replay) ; `SCmdEvent` ~12 000–17 000/replay |
| 4 | Déserteurs | **Oui** | `SGameUserLeaveEvent.m_leaveReason` : `0` (fin normale, 155) et `11` (départ en cours de partie, 8, tous avant les 30 dernières secondes) |
| 5 | Niveau | **Réduit** | `m_highestLeague` = 0 partout (inutilisable). Restent `m_heroMasteryTiers` (tier 1–4 par héros) et `m_hasActiveBoost` |
| 6 | Habitudes de chat | **Oui, sans extraction** | le chat est déjà dans `matches.data.messages` (type 0 = chat : 44 191 messages ; autres types = pings, annonces) |

**L'idée 2 est abandonnée** : les champs existent mais ne sont jamais renseignés.

## Décisions

### A. Cosmétiques comme signal de comptes liés (idée 1)
- Signal « cosmétiques » dans `similarity.rs` : recouvrement des skins/montures/annonceurs **portés en
  partie** par les deux comptes, pondéré par la rareté de l'objet (un cosmétique porté par beaucoup de
  comptes ne prouve rien, même logique que le nom de profil générique).
- Force : **moyenne**, jamais HAUT seul. **[mon choix]** Poids et seuils réglés sur une sonde du corpus.
- Aucune migration : donnée déjà en `data`. Requêtes agrégées à mesurer avant d'ajouter un index.

### B. Déserteurs (idée 4)
- Par compte : nombre de parties, départs en cours de partie (`m_leaveReason` = 11 et avant la fin), taux.
- Stockage : colonne `match_players.left_early BOOLEAN` (migration, `PARSER_VERSION` 4).
- Affichage : taux sur la fiche joueur, mention dans le brief de scouting au-dessus d'un seuil
  **[mon choix]** ≥ 3 départs et ≥ 5 % des parties. Le départ d'un joueur coupé par le jeu (déconnexion)
  n'est pas distinguable d'un abandon volontaire : l'écran le dit.

### C. Habitudes de chat (idée 6)
- **Mesures uniquement** : messages par partie, part équipe / tous (`recipient`), moment typique
  (début, milieu, fin), mots récurrents (liste filtrée des mots vides). **Aucun score de toxicité,
  aucun classement.**
- Calcul à la lecture depuis `matches.data.messages` pour un seul compte (pas de pré-calcul massif) ;
  cache si la mesure dépasse le budget API (p95 < 100 ms).
- Affichage : section « Chat » sur la fiche joueur ; une ligne dans le brief de scouting de l'adversaire.
  Les textes ne sont pas exposés, seulement les mesures et les mots récurrents.

### D. Empreinte clavier (idée 3)
- Par compte : touches les plus utilisées et répartition (sans contenu de chat), cadence d'actions
  (commandes par minute) par héros.
- Signal faible de comptes liés ; **[mon choix]** affiché mais pas dans le score tant qu'une sonde n'a
  pas montré qu'il discrimine (comparer des comptes confirmés liés à des comptes sans lien).
- Stockage : agrégat par `(match, toon)` calculé à l'ingestion dans une colonne JSONB dédiée, pas
  l'événement brut (17 000 événements/replay).

### E. Niveau et progression (idée 5, réduite)
- `m_heroMasteryTiers` : tier de maîtrise du héros joué, par partie ; `m_hasActiveBoost` : booléen.
- Affichage : tier du héros sur la fiche joueur. **[mon choix]** pas de signal de comptes liés (un tier
  dépend du temps de jeu, pas de la personne).

## Ordre proposé
1. C (chat) : aucune extraction, valeur immédiate. 2. B (déserteurs). 3. A (cosmétiques). 4. E.
5. D (clavier, à mesurer d'abord). Chaque lot a son plan, sa PR, sa mise à jour de doc.

## Hors périmètre
- Toxicité, insultes, classification IA du chat (écartés par l'opérateur).
- Groupes/premade (idée 2) : données absentes.
- Exposer le texte des messages.

## Questions ouvertes pour l'opérateur
1. Ordre C → B → A → E → D acceptable, ou une idée à passer en premier ?
2. Seuil de mention des déserteurs dans le scouting : ≥ 3 départs et ≥ 5 % ?
3. Cosmétiques : signal moyen, jamais HAUT seul ?
4. Clavier : affiché mais hors score tant qu'une sonde ne le justifie pas ?
