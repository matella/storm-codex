# APM par partie et par joueur — design

> Statut : **demandé par l'opérateur le 2026-11-08** (« ajouter des APM pour chaque game pour chaque
> joueur, et une moyenne par joueur comme stat »). Réduit le lot D « empreinte clavier » de
> `2026-11-08-signaux-replay-supplementaires-design.md` à ce qui a été demandé : l'APM. Les choix
> marqués **[mon choix]** restent à confirmer.

## Constat (vérifié le 2026-11-08 sur un vrai replay, 21,6 min, 10 joueurs)
Le flux `game` contient `SCmdEvent` (~26 000, avec `_userid`) : déplacements (clic droit,
`m_abil` nul), attaques, aptitudes. Par joueur : 1 300 à 4 400 `SCmdEvent`, soit 60 à 200 par minute.
`STriggerKeyPressedEvent` (touches) répète les touches maintenues, `SCmdUpdateTargetPointEvent` suit la
souris pendant un ordre : ni l'un ni l'autre ne mesure des actions.

## Décisions
1. **Définition [mon choix]** : APM = nombre de `SCmdEvent` du joueur à partir du début de la partie
   (`loop_game_start`) ÷ durée de la partie en minutes (`matches.length`). Un joueur parti avant la fin
   est donc sous-évalué ; l'écran ne le corrige pas. Heroes of the Storm n'a pas d'APM officiel : la valeur
   n'est comparable qu'entre parties de ce système.
2. **Stockage** : `match_players.apm` (réel), calculé à l'ingestion (`PARSER_VERSION` 4). NULL = non
   re-traité ou durée inconnue.
3. **Moyenne par joueur** : moyenne des `apm` non NULL du compte, arrondie à l'entier ; `apm_games` dit sur
   combien de parties elle repose. Aucune pondération par héros ni filtre de durée **[mon choix]**.
4. **Affichage** : tuile « Avg APM » sur la fiche joueur, pastille APM sur chaque partie récente, colonne
   « APM » dans le tableau de score du match.
5. **Hors score de comptes liés** : un APM dépend du héros et de la partie ; pas de signal d'identité.

## Hors périmètre
Touches les plus utilisées, cadence par héros, empreinte clavier (reste du lot D).
