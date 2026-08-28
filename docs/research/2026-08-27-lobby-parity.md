# Parité du parser autonome de lobby — rapport go/no-go

**Mesure exécutée le 2026-08-28** sur l'archive complète du box (3 322 replays, 25 builds de 2024 à
2026), copiée en lecture seule via `docker cp` puis supprimée. Harnais :
`crates/storm-lobby/examples/parity.rs` — ce fichier produit **lui-même** tous les chiffres de ce
rapport (ventilation par mode, diagnostic sur-capture/sous-capture, divergences nommées) ; aucun
script jetable n'intervient plus dans cette mesure. Référence de vérité : le parse complet
(`storm-stats`), qui connaît la composition réelle de chaque partie.

> Note de méthode. Une version antérieure de ce rapport calculait sa base de comparaison en ne
> retranchant que les replays rejetés par `storm-stats`, laissant les échecs d'ouverture / de blob
> absent au dénominateur sans les compter nulle part (5 replays disparaissaient silencieusement du
> calcul). Le harnais compte désormais ces cas dans `non-évaluables`, retranché de la base comme
> `écartés (stats)`. L'archive a par ailleurs grandi de 8 replays entre la copie initiale et cette
> mesure (3 314 → 3 322 ; Storm League 1 970 → 1 978) — l'opérateur a joué entre-temps, sans
> conséquence sur le verdict.

## Verdict

**GO sur les modes matchmakés. Les parties personnalisées sont hors garantie.**

Sur Storm League, ARAM et Quick Match — 2 710 replays — le parser retrouve **100 % des BattleTags
et 100 % des équipes**, sans une seule exception. Le critère du plan (≥ 99 %) est dépassé sur le
périmètre qui porte 83,9 % de la base de comparaison.

Les parties personnalisées échouent et ne doivent pas être présentées comme fiables.

## Le chiffre global, et pourquoi il induit en erreur

La mesure, tous modes confondus, donne :

```
replays vus          : 3322
non-évaluables       : 5 (ouverture/blob)
écartés (stats)      : 86
base de comparaison  : 3231
battletags exacts    : 3108 (96.19 %)
erreurs de parse     : 0
équipes exactes      : 2825 / 3108 évaluables (90.89 %)
```

96,19 % < 99 % : **no-go** au sens littéral du critère, sur la base de comparaison entière
(3 231 replays). Mais la ventilation par build montrait déjà, avant cette mesure, une anomalie —
les builds *récents* échouaient plus que les anciens. Une dégradation de format aurait produit
l'inverse. Le facteur n'était donc pas le build ; c'est le mode de jeu, ci-dessous.

Ventilation par mode de jeu (dénominateurs séparés : « évalués » n'est pas « battletags exacts ») :

| Mode | Évalués | BattleTags exacts | Équipes exactes (parmi les BattleTags exacts) |
|---|---|---|---|
| 50091 — Storm League | 1 978 | 1 978 (**100,00 %**) | 1 978 (**100,00 %**) |
| 50101 — ARAM | 729 | 729 (**100,00 %**) | 729 (**100,00 %**) |
| 50001 — Quick Match | 3 | 3 (**100,00 %**) | 3 (**100,00 %**) |
| −1 — Personnalisée | 521 | 398 (76,39 %) | 115 (28,89 %) |

Les modes matchmakés (Storm League + ARAM + Quick Match) totalisent 1 978 + 729 + 3 = **2 710**
replays, soit **83,9 %** de la base de comparaison (2 710 / 3 231). Les personnalisées en portent
**16,1 %** (521 / 3 231) — même dénominateur, nommé ici explicitement pour éviter l'erreur d'une
version antérieure de ce rapport, qui avait rapporté ces parts (87 % / 13 %) en les calculant sur
les seuls replays déjà réussis (2 702 / 3 100) plutôt que sur la base de comparaison entière.

Le 96,19 % global est **entièrement** produit par les parties personnalisées. C'est un cas d'école
de moyenne qui masque deux populations : mélanger les deux aurait conduit à abandonner une capacité
parfaite là où elle sert.

## Mode d'échec : sur-capture, jamais perte

Vérifié de façon **exhaustive** sur les 3 231 replays de la base de comparaison, et non sur un
échantillon de divergences :

```
mode d'échec (sur la base entière, 3231 replays)
  sur-capture (10 vrais + occupants en trop) : 123 (3.81 %)
  sous-capture ou décodage faux               : 0 (0.00 %)
```

Le parser ne rate **jamais** un joueur et ne produit **jamais** de décodage malformé : la totalité
des 123 divergences de BattleTags de l'archive sont des sur-captures — les 10 vrais joueurs
**plus** des personnes en trop. Aucune occurrence de perte ou de décodage faux dans toute
l'archive.

Ces personnes en trop sont de vrais comptes présents dans le lobby mais qui ne jouent pas — des
**observateurs**. Extrait de diagnostic, nommé, sur trois replays fautifs :

```
2088f5ef… — 12 battletags décodés
    EN TROP -> RydzuSA#2791
    EN TROP -> Xardas182#21665
1e923b98… — 13 battletags décodés
    EN TROP -> RydzuSA#2791
    EN TROP -> TLHasuObs#2433
    EN TROP -> Bahamut#11165
a1f867de… — 11 battletags décodés
    EN TROP -> Krankle123#2459
```

`TLHasuObs` porte « Obs » dans son nom : c'est un compte d'observation. Le blob de lobby liste les
occupants du lobby, pas les joueurs de la partie — distinction sans conséquence en matchmaking (où
les deux ensembles coïncident), déterminante en partie personnalisée.

## Équipes : deux échecs de nature différente

La déduction 5+5 par l'ordre est **exacte à 100 %** en matchmaking. En personnalisée, sur les 398
replays dont les 10 BattleTags sont pourtant corrects :

- 115 corrects (28,9 %)
- **283 faux** (les 10 joueurs sont bons mais l'ordre ne reconstruit pas la bonne équipe)

Ce résidu n'est pas un signe à corriger, c'est une information absente. En lobby personnalisé, les
joueurs changent de slot et d'équipe avant le lancement, et l'ordre du blob reflète l'ordre
d'arrivée, pas la composition finale.

## Limite structurelle à connaître

Le blob **ne porte pas le mode de jeu** (établi en tâche 1). Le parser ne peut donc pas savoir
lui-même s'il lit un lobby matchmaké ou personnalisé. Conséquence : sur une partie personnalisée
comptant exactement 10 occupants et aucun observateur, il assignera des équipes avec la même
assurance qu'en Storm League — et se trompera dans la majorité de ces cas (283/398 ≈ 71 % des
personnalisées à BattleTags corrects).

## Recommandations (décision opérateur)

1. **Conserver la déduction d'équipe.** Elle est parfaite sur 2 710 parties matchmakées. La
   supprimer pour se prémunir des personnalisées reviendrait à dégrader 83,9 % des cas pour 16,1 %.
2. **Ne pas tenter de filtrer les observateurs dans le crate.** Rien dans le blob ne les distingue
   d'un joueur ; toute heuristique serait une supposition non mesurée.
3. **Traiter « ≠ 10 occupants » comme le signal disponible.** C'est déjà la règle du parser
   (`team: None` hors du cas 10 pile) et elle attrape les personnalisées avec observateurs — testée
   désormais par `crates/storm-lobby/tests/team_rule.rs` sur un blob synthétique.
4. **Afficher la réserve côté produit** pour le cas résiduel — une personnalisée à 10 occupants
   pile. Le companion peut proposer une inversion manuelle des deux équipes d'un clic ; c'est
   suffisant, puisque le seul mode d'erreur restant qui soit réparable est l'inversion.

## Pistes non implémentées

| Piste | Coût | Risque |
|---|---|---|
| Repérer un marqueur d'observateur dans le blob | reverse-engineering supplémentaire, durée imprévisible | échec probable : la tâche 1 n'a trouvé aucun champ structurel exploitable |
| Résoudre les équipes via l'archive (qui joue habituellement avec qui) | moyen | ne marche que pour des joueurs déjà connus ; inopérant sur des inconnus |
| Corréler les hashes `.s2ma` du blob à la carte via l'archive | faible | indépendant du problème d'équipe ; supprimerait la saisie manuelle de la carte |

## Reproduire

Une seule commande régénère l'intégralité de ce rapport — le chiffre global, la ventilation par
mode, le diagnostic sur-capture/sous-capture, la ventilation par build et les divergences nommées :

```bash
cargo run --release -p storm-lobby --example parity -- <dossier de replays>
```

Un `max` optionnel limite le nombre de fichiers scannés (`... -- <dossier> 500`) ; une valeur non
numérique fait échouer l'outil plutôt que d'être avalée en silence.

Rodage rapide sur le corpus committé (aucune archive externe requise) :
`cargo run --release -p storm-lobby --example parity -- crates/storm-replay/tests/data` →
3/3 exacts (100 %).
