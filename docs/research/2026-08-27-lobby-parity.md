# Parité du parser autonome de lobby — rapport go/no-go

**Mesure exécutée le 2026-08-28** sur l'archive complète du box (3 314 replays, 25 builds de 2024 à
2026), copiée en lecture seule via `docker cp` puis supprimée. Harnais :
`crates/storm-lobby/examples/parity.rs`. Référence de vérité : le parse complet (`storm-stats`),
qui connaît la composition réelle de chaque partie.

## Verdict

**GO sur les modes matchmakés. Les parties personnalisées sont hors garantie.**

Sur Storm League, ARAM et Quick Match — 2 702 replays — le parser retrouve **100 % des BattleTags
et 100 % des équipes**, sans une seule exception. Le critère du plan (≥ 99 %) est dépassé sur le
périmètre qui porte 87 % de l'archive.

Les parties personnalisées échouent et ne doivent pas être présentées comme fiables.

## Le chiffre global, et pourquoi il induit en erreur

La première mesure, tous modes confondus, donnait :

```
replays vus          : 3314
écartés (stats)      : 86
base de comparaison  : 3228
battletags exacts    : 3100 (96.03 %)
erreurs de parse     : 0
équipes exactes      : 2817 / 3100 évaluables (90.87 %)
```

96,03 % < 99 % : **no-go** au sens littéral du critère. Mais la ventilation par build montrait une
anomalie — les builds *récents* échouaient plus que les anciens (97771 : 90/111 ; 97650 : 100/117 ;
93810 : 432/432). Une dégradation de format aurait produit l'inverse. Le facteur n'était donc pas
le build.

Ventilation par mode de jeu :

| Mode | Évalués | BattleTags exacts | Équipes exactes |
|---|---|---|---|
| 50091 — Storm League | 1970 | **1970 (100,00 %)** | **1970 (100,00 %)** |
| 50101 — ARAM | 729 | **729 (100,00 %)** | **729 (100,00 %)** |
| 50001 — Quick Match | 3 | 3 (100,00 %) | 3 (100,00 %) |
| −1 — Personnalisée | 521 / 398 | 398 (76,39 %) | 115 (28,89 %) |

Le 96 % global était entièrement produit par les parties personnalisées. C'est un cas d'école de
moyenne qui masque deux populations : mélanger les deux aurait conduit à abandonner une capacité
parfaite là où elle sert.

## Mode d'échec : sur-capture, jamais perte

Vérifié de façon **exhaustive** sur les 3 314 replays, et non sur un échantillon de divergences :
le parser ne rate **jamais** un joueur et ne produit **jamais** de décodage malformé. Quand il se
trompe, il décode 11, 12 ou 13 BattleTags au lieu de 10 — les 10 vrais joueurs **plus** des
personnes en trop. Aucune occurrence inverse dans toute l'archive.

Ces personnes en trop sont de vrais comptes présents dans le lobby mais qui ne jouent pas — des
**observateurs**. Extrait de diagnostic sur trois replays fautifs :

```
=== 2088f5ef… — 12 décodés / 10 réels
   0 EN TROP -> RydzuSA#2791
   6 EN TROP -> Xardas182#21665
=== 1e923b98… — 13 décodés / 10 réels
   3 EN TROP -> RydzuSA#2791
   7 EN TROP -> TLHasuObs#2433
   9 EN TROP -> Bahamut#11165
=== a1f867de… — 11 décodés / 10 réels
  10 EN TROP -> Krankle123#2459
```

`TLHasuObs` porte « Obs » dans son nom : c'est un compte d'observation. Le blob de lobby liste les
occupants du lobby, pas les joueurs de la partie — distinction sans conséquence en matchmaking (où
les deux ensembles coïncident), déterminante en partie personnalisée.

## Équipes : deux échecs de nature différente

La déduction 5+5 par l'ordre est **exacte à 100 %** en matchmaking. En personnalisée, sur les 398
replays dont les 10 BattleTags sont pourtant corrects :

- 115 corrects (28,9 %)
- **15 inversions franches** (les 10 joueurs faux : les deux équipes sont simplement permutées)
- **268 dispersés** (l'ordre ne porte aucune information d'équipe)

Les 268 « dispersés » sont l'obstacle réel : ce n'est pas un signe à corriger, c'est une
information absente. En lobby personnalisé, les joueurs changent de slot et d'équipe avant le
lancement, et l'ordre du blob reflète l'ordre d'arrivée, pas la composition finale.

## Limite structurelle à connaître

Le blob **ne porte pas le mode de jeu** (établi en tâche 1). Le parser ne peut donc pas savoir
lui-même s'il lit un lobby matchmaké ou personnalisé. Conséquence : sur une partie personnalisée
comptant exactement 10 occupants et aucun observateur, il assignera des équipes avec la même
assurance qu'en Storm League — et se trompera dans environ deux tiers des cas.

## Recommandations (décision opérateur)

1. **Conserver la déduction d'équipe.** Elle est parfaite sur 2 702 parties matchmakées. La
   supprimer pour se prémunir des personnalisées reviendrait à dégrader 87 % des cas pour 13 %.
2. **Ne pas tenter de filtrer les observateurs dans le crate.** Rien dans le blob ne les distingue
   d'un joueur ; toute heuristique serait une supposition non mesurée.
3. **Traiter « ≠ 10 occupants » comme le signal disponible.** C'est déjà la règle du parser
   (`team: None` hors du cas 10 pile) et elle attrape les personnalisées avec observateurs.
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

```bash
cargo run --release -p storm-lobby --example parity -- <dossier de replays>
```

Rodage sur le corpus committé : `crates/storm-replay/tests/data` → 3/3 exacts (100 %).
